use std::collections::HashSet;
use std::path::PathBuf;

use roxmltree::{Document, Node};

use crate::error::{Error, Result};
use crate::project::{AudioClip, AudioTrack, Project, SampleRef};
use crate::time::{Tempo, TempoPoint};

/// Parse the decompressed Ableton Live `.als` XML into a [`Project`] model.
pub fn parse(xml: &str) -> Result<Project> {
    let doc = Document::parse(xml)?;
    let root = doc.root_element();

    if root.tag_name().name() != "Ableton" {
        return Err(Error::Malformed(format!(
            "expected root <Ableton>, got <{}>",
            root.tag_name().name()
        )));
    }

    let live_set = child(&root, "LiveSet").ok_or(Error::MissingElement("LiveSet"))?;

    let tempo = parse_tempo(&live_set)?;
    let audio_tracks = parse_audio_tracks(&live_set)?;
    let all_sample_refs = collect_all_sample_refs(&live_set);

    Ok(Project {
        tempo,
        audio_tracks,
        all_sample_refs,
    })
}

/// Walk every `<SampleRef>` in the document, parse the underlying `<FileRef>`,
/// and dedupe by [`SampleRef::identity`]. Discovery order is preserved.
fn collect_all_sample_refs(live_set: &Node) -> Vec<SampleRef> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for node in live_set.descendants().filter(|n| n.has_tag_name("SampleRef")) {
        if let Some(sample) = parse_sample_ref_node(&node) {
            if seen.insert(sample.identity().to_string()) {
                out.push(sample);
            }
        }
    }
    out
}

fn parse_tempo(live_set: &Node) -> Result<Tempo> {
    let master_track = child(live_set, "MasterTrack").ok_or(Error::MissingElement("MasterTrack"))?;
    let device_chain =
        child(&master_track, "DeviceChain").ok_or(Error::MissingElement("MasterTrack/DeviceChain"))?;
    let mixer = child(&device_chain, "Mixer").ok_or(Error::MissingElement("MasterTrack/DeviceChain/Mixer"))?;
    let tempo_node = child(&mixer, "Tempo").ok_or(Error::MissingElement("MasterTrack/DeviceChain/Mixer/Tempo"))?;

    let manual_bpm = child_value_attr(&tempo_node, "Manual", "Value")
        .ok_or(Error::MissingElement("Tempo/Manual"))?
        .parse::<f64>()
        .map_err(|e| Error::Malformed(format!("invalid tempo Manual value: {e}")))?;

    let automation_target_id = child(&tempo_node, "AutomationTarget")
        .and_then(|n| n.attribute("Id"))
        .map(|s| s.to_string());

    if let Some(target_id) = automation_target_id {
        if let Some(events) = find_tempo_automation_events(&master_track, &target_id)? {
            return Ok(Tempo::from_automation_events(events, manual_bpm));
        }
    }

    Ok(Tempo::Constant(manual_bpm))
}

/// Locate FloatEvents on a tempo automation envelope on the MasterTrack
/// whose target id matches `target_id`.
fn find_tempo_automation_events(
    master_track: &Node,
    target_id: &str,
) -> Result<Option<Vec<TempoPoint>>> {
    let envelopes_root = match child(master_track, "AutomationEnvelopes") {
        Some(n) => n,
        None => return Ok(None),
    };
    let envelopes = match child(&envelopes_root, "Envelopes") {
        Some(n) => n,
        None => return Ok(None),
    };

    for env in envelopes.children().filter(|n| n.is_element()) {
        if env.tag_name().name() != "AutomationEnvelope" {
            continue;
        }
        let pointee = env
            .descendants()
            .find(|n| n.has_tag_name("PointeeId"))
            .and_then(|n| n.attribute("Value"));
        if pointee != Some(target_id) {
            continue;
        }

        let events_node = match env
            .descendants()
            .find(|n| n.has_tag_name("Events") && n.parent().is_some_and(|p| p.has_tag_name("Automation")))
        {
            Some(n) => n,
            None => return Ok(Some(Vec::new())),
        };

        let mut points = Vec::new();
        for event in events_node.children().filter(|n| n.is_element()) {
            if event.tag_name().name() != "FloatEvent" {
                continue;
            }
            let beat = event
                .attribute("Time")
                .and_then(|s| s.parse::<f64>().ok())
                .ok_or_else(|| Error::Malformed("FloatEvent missing Time".into()))?;
            let bpm = event
                .attribute("Value")
                .and_then(|s| s.parse::<f64>().ok())
                .ok_or_else(|| Error::Malformed("FloatEvent missing Value".into()))?;
            points.push(TempoPoint { beat, bpm });
        }
        return Ok(Some(points));
    }

    Ok(None)
}

fn parse_audio_tracks(live_set: &Node) -> Result<Vec<AudioTrack>> {
    let tracks_node = child(live_set, "Tracks").ok_or(Error::MissingElement("Tracks"))?;
    let mut audio_tracks = Vec::new();

    for track in tracks_node.children().filter(|n| n.is_element()) {
        if track.tag_name().name() != "AudioTrack" {
            continue;
        }
        audio_tracks.push(parse_audio_track(&track)?);
    }

    Ok(audio_tracks)
}

fn parse_audio_track(track: &Node) -> Result<AudioTrack> {
    let id = track.attribute("Id").unwrap_or("").to_string();
    let name = child(track, "Name")
        .and_then(|n| child_value_attr(&n, "EffectiveName", "Value"))
        .unwrap_or_default();

    let clips = parse_track_clips(track)?;

    Ok(AudioTrack { id, name, clips })
}

fn parse_track_clips(track: &Node) -> Result<Vec<AudioClip>> {
    // AudioTrack > DeviceChain > MainSequencer > Sample > ArrangerAutomation > Events > AudioClip*
    let device_chain = match child(track, "DeviceChain") {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };
    let main_sequencer = match child(&device_chain, "MainSequencer") {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };
    let sample = match child(&main_sequencer, "Sample") {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };
    let arranger = match child(&sample, "ArrangerAutomation") {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };
    let events = match child(&arranger, "Events") {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let mut clips = Vec::new();
    for clip_node in events.children().filter(|n| n.is_element()) {
        if clip_node.tag_name().name() != "AudioClip" {
            continue;
        }
        clips.push(parse_audio_clip(&clip_node)?);
    }
    Ok(clips)
}

fn parse_audio_clip(clip: &Node) -> Result<AudioClip> {
    let start_beats = child_value_attr(clip, "CurrentStart", "Value")
        .ok_or(Error::MissingElement("AudioClip/CurrentStart"))?
        .parse::<f64>()
        .map_err(|e| Error::Malformed(format!("invalid CurrentStart: {e}")))?;
    let end_beats = child_value_attr(clip, "CurrentEnd", "Value")
        .ok_or(Error::MissingElement("AudioClip/CurrentEnd"))?
        .parse::<f64>()
        .map_err(|e| Error::Malformed(format!("invalid CurrentEnd: {e}")))?;
    let name = child_value_attr(clip, "Name", "Value").unwrap_or_default();

    let sample = parse_sample_ref(clip)?;

    Ok(AudioClip {
        name,
        start_beats,
        end_beats,
        sample,
    })
}

fn parse_sample_ref(clip: &Node) -> Result<SampleRef> {
    let sample_ref = child(clip, "SampleRef").ok_or(Error::MissingElement("AudioClip/SampleRef"))?;
    parse_sample_ref_node(&sample_ref).ok_or(Error::MissingElement("SampleRef/FileRef"))
}

fn parse_sample_ref_node(sample_ref: &Node) -> Option<SampleRef> {
    let file_ref = child(sample_ref, "FileRef")?;

    let absolute_path = child_value_attr(&file_ref, "Path", "Value")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let relative_path = child_value_attr(&file_ref, "RelativePath", "Value")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let name = child_value_attr(&file_ref, "Name", "Value")
        .or_else(|| {
            absolute_path
                .as_ref()
                .or(relative_path.as_ref())
                .and_then(|p| p.file_name().and_then(|s| s.to_str()).map(|s| s.to_string()))
        })
        .unwrap_or_default();

    Some(SampleRef {
        name,
        absolute_path,
        relative_path,
    })
}

fn child<'a, 'input>(node: &Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name() == name)
}

fn child_value_attr(node: &Node, child_name: &str, attr: &str) -> Option<String> {
    child(node, child_name)
        .and_then(|n| n.attribute(attr))
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN_PROJECT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Ableton>
  <LiveSet>
    <Tracks>
      <AudioTrack Id="1">
        <Name><EffectiveName Value="Track A"/></Name>
        <DeviceChain>
          <MainSequencer>
            <Sample>
              <ArrangerAutomation>
                <Events>
                  <AudioClip Id="100" Time="0">
                    <CurrentStart Value="0"/>
                    <CurrentEnd Value="32"/>
                    <Name Value="Clip A"/>
                    <SampleRef>
                      <FileRef>
                        <RelativePath Value="Samples/a.wav"/>
                        <Path Value="C:/proj/Samples/a.wav"/>
                        <Name Value="a.wav"/>
                      </FileRef>
                    </SampleRef>
                  </AudioClip>
                </Events>
              </ArrangerAutomation>
            </Sample>
          </MainSequencer>
        </DeviceChain>
      </AudioTrack>
    </Tracks>
    <MasterTrack>
      <DeviceChain>
        <Mixer>
          <Tempo>
            <Manual Value="120"/>
            <AutomationTarget Id="8"/>
          </Tempo>
        </Mixer>
      </DeviceChain>
    </MasterTrack>
  </LiveSet>
</Ableton>"#;

    #[test]
    fn parse_minimal_project() {
        let project = parse(MIN_PROJECT).unwrap();
        assert_eq!(project.tempo, Tempo::Constant(120.0));
        assert_eq!(project.audio_tracks.len(), 1);
        let track = &project.audio_tracks[0];
        assert_eq!(track.id, "1");
        assert_eq!(track.name, "Track A");
        assert_eq!(track.clips.len(), 1);
        let clip = &track.clips[0];
        assert_eq!(clip.name, "Clip A");
        assert_eq!(clip.start_beats, 0.0);
        assert_eq!(clip.end_beats, 32.0);
        assert_eq!(clip.sample.name, "a.wav");
        assert_eq!(
            clip.sample.absolute_path.as_deref(),
            Some(std::path::Path::new("C:/proj/Samples/a.wav"))
        );
    }

    #[test]
    fn rejects_non_ableton_root() {
        let xml = "<NotAbleton/>";
        let err = parse(xml).unwrap_err();
        assert!(matches!(err, Error::Malformed(_)));
    }

    #[test]
    fn errors_on_missing_live_set() {
        let xml = "<Ableton/>";
        let err = parse(xml).unwrap_err();
        assert!(matches!(err, Error::MissingElement("LiveSet")));
    }

    #[test]
    fn errors_on_invalid_xml() {
        let err = parse("<<<not xml>>>").unwrap_err();
        assert!(matches!(err, Error::Xml(_)));
    }

    #[test]
    fn parses_tempo_automation_when_envelope_present() {
        let xml = r#"<?xml version="1.0"?>
<Ableton>
  <LiveSet>
    <Tracks/>
    <MasterTrack>
      <DeviceChain>
        <Mixer>
          <Tempo>
            <Manual Value="120"/>
            <AutomationTarget Id="8"/>
          </Tempo>
        </Mixer>
      </DeviceChain>
      <AutomationEnvelopes>
        <Envelopes>
          <AutomationEnvelope Id="0">
            <EnvelopeTarget><PointeeId Value="8"/></EnvelopeTarget>
            <Automation>
              <Events>
                <FloatEvent Id="0" Time="-63072000" Value="120"/>
                <FloatEvent Id="1" Time="100" Value="140"/>
              </Events>
            </Automation>
          </AutomationEnvelope>
        </Envelopes>
      </AutomationEnvelopes>
    </MasterTrack>
  </LiveSet>
</Ableton>"#;
        let project = parse(xml).unwrap();
        assert!(matches!(project.tempo, Tempo::Automated(_)));
    }

    #[test]
    fn ignores_unrelated_envelope() {
        // Envelope target points elsewhere → falls back to constant tempo.
        let xml = r#"<?xml version="1.0"?>
<Ableton>
  <LiveSet>
    <Tracks/>
    <MasterTrack>
      <DeviceChain>
        <Mixer>
          <Tempo>
            <Manual Value="100"/>
            <AutomationTarget Id="8"/>
          </Tempo>
        </Mixer>
      </DeviceChain>
      <AutomationEnvelopes>
        <Envelopes>
          <AutomationEnvelope Id="0">
            <EnvelopeTarget><PointeeId Value="999"/></EnvelopeTarget>
            <Automation><Events><FloatEvent Id="0" Time="0" Value="50"/></Events></Automation>
          </AutomationEnvelope>
        </Envelopes>
      </AutomationEnvelopes>
    </MasterTrack>
  </LiveSet>
</Ableton>"#;
        let project = parse(xml).unwrap();
        assert_eq!(project.tempo, Tempo::Constant(100.0));
    }

    #[test]
    fn skips_non_audio_tracks() {
        let xml = r#"<?xml version="1.0"?>
<Ableton>
  <LiveSet>
    <Tracks>
      <ReturnTrack Id="2"/>
      <MidiTrack Id="3"/>
    </Tracks>
    <MasterTrack>
      <DeviceChain>
        <Mixer>
          <Tempo><Manual Value="120"/></Tempo>
        </Mixer>
      </DeviceChain>
    </MasterTrack>
  </LiveSet>
</Ableton>"#;
        let project = parse(xml).unwrap();
        assert!(project.audio_tracks.is_empty());
    }

    #[test]
    fn audio_track_with_no_device_chain_yields_no_clips() {
        let xml = r#"<?xml version="1.0"?>
<Ableton><LiveSet>
  <Tracks>
    <AudioTrack Id="1"><Name><EffectiveName Value="t"/></Name></AudioTrack>
  </Tracks>
  <MasterTrack><DeviceChain><Mixer><Tempo><Manual Value="120"/></Tempo></Mixer></DeviceChain></MasterTrack>
</LiveSet></Ableton>"#;
        let project = parse(xml).unwrap();
        assert_eq!(project.audio_tracks.len(), 1);
        assert!(project.audio_tracks[0].clips.is_empty());
    }

    #[test]
    fn audio_track_missing_intermediate_nodes_yields_no_clips() {
        // DeviceChain present but MainSequencer absent. Same applies for the
        // deeper missing-node paths; we just want to exercise the early-exit.
        let xml = r#"<?xml version="1.0"?>
<Ableton><LiveSet>
  <Tracks>
    <AudioTrack Id="1">
      <Name><EffectiveName Value="t"/></Name>
      <DeviceChain/>
    </AudioTrack>
  </Tracks>
  <MasterTrack><DeviceChain><Mixer><Tempo><Manual Value="120"/></Tempo></Mixer></DeviceChain></MasterTrack>
</LiveSet></Ableton>"#;
        let project = parse(xml).unwrap();
        assert!(project.audio_tracks[0].clips.is_empty());
    }

    #[test]
    fn ignores_non_envelope_siblings_and_non_float_events() {
        // The Envelopes block contains a non-AutomationEnvelope child, and the
        // matching envelope's Events block contains a non-FloatEvent child.
        let xml = r#"<?xml version="1.0"?>
<Ableton><LiveSet>
  <Tracks/>
  <MasterTrack>
    <DeviceChain><Mixer><Tempo><Manual Value="100"/><AutomationTarget Id="8"/></Tempo></Mixer></DeviceChain>
    <AutomationEnvelopes>
      <Envelopes>
        <Decoy/>
        <AutomationEnvelope Id="0">
          <EnvelopeTarget><PointeeId Value="8"/></EnvelopeTarget>
          <Automation>
            <Events>
              <NotAFloatEvent Time="0" Value="1"/>
              <FloatEvent Time="0" Value="100"/>
              <FloatEvent Time="50" Value="120"/>
            </Events>
          </Automation>
        </AutomationEnvelope>
      </Envelopes>
    </AutomationEnvelopes>
  </MasterTrack>
</LiveSet></Ableton>"#;
        let project = parse(xml).unwrap();
        assert!(matches!(project.tempo, Tempo::Automated(_)));
    }

    #[test]
    fn envelope_present_but_envelopes_missing_falls_back_to_constant() {
        let xml = r#"<?xml version="1.0"?>
<Ableton><LiveSet>
  <Tracks/>
  <MasterTrack>
    <DeviceChain><Mixer><Tempo><Manual Value="123"/><AutomationTarget Id="8"/></Tempo></Mixer></DeviceChain>
    <AutomationEnvelopes/>
  </MasterTrack>
</LiveSet></Ableton>"#;
        let project = parse(xml).unwrap();
        assert_eq!(project.tempo, Tempo::Constant(123.0));
    }

    #[test]
    fn non_audio_clip_event_children_are_skipped() {
        let xml = r#"<?xml version="1.0"?>
<Ableton><LiveSet>
  <Tracks>
    <AudioTrack Id="1">
      <Name><EffectiveName Value="t"/></Name>
      <DeviceChain><MainSequencer><Sample><ArrangerAutomation><Events>
        <NotAClip/>
        <AudioClip Id="1" Time="0">
          <CurrentStart Value="0"/>
          <CurrentEnd Value="4"/>
          <Name Value="ok"/>
          <SampleRef><FileRef><Path Value="/x.wav"/><Name Value="x.wav"/></FileRef></SampleRef>
        </AudioClip>
      </Events></ArrangerAutomation></Sample></MainSequencer></DeviceChain>
    </AudioTrack>
  </Tracks>
  <MasterTrack><DeviceChain><Mixer><Tempo><Manual Value="120"/></Tempo></Mixer></DeviceChain></MasterTrack>
</LiveSet></Ableton>"#;
        let project = parse(xml).unwrap();
        assert_eq!(project.audio_tracks[0].clips.len(), 1);
    }

    #[test]
    fn all_sample_refs_collects_clip_and_non_clip_refs() {
        // One SampleRef is on a clip, one lives under a non-clip device
        // (a faux Sampler) — the descendants pass should pick up both.
        let xml = r#"<?xml version="1.0"?>
<Ableton><LiveSet>
  <Tracks>
    <AudioTrack Id="1">
      <Name><EffectiveName Value="t"/></Name>
      <DeviceChain><MainSequencer><Sample><ArrangerAutomation><Events>
        <AudioClip Id="1" Time="0">
          <CurrentStart Value="0"/>
          <CurrentEnd Value="4"/>
          <Name Value="c"/>
          <SampleRef><FileRef><Path Value="/clip.wav"/><Name Value="clip.wav"/></FileRef></SampleRef>
        </AudioClip>
      </Events></ArrangerAutomation></Sample></MainSequencer></DeviceChain>
    </AudioTrack>
    <MidiTrack Id="2">
      <DeviceChain><Devices><MultiSampler>
        <Player><MultiSampleMap><SampleParts><MultiSamplePart>
          <SampleRef><FileRef><Path Value="/inst.wav"/><Name Value="inst.wav"/></FileRef></SampleRef>
        </MultiSamplePart></SampleParts></MultiSampleMap></Player>
      </MultiSampler></Devices></DeviceChain>
    </MidiTrack>
  </Tracks>
  <MasterTrack><DeviceChain><Mixer><Tempo><Manual Value="120"/></Tempo></Mixer></DeviceChain></MasterTrack>
</LiveSet></Ableton>"#;
        let project = parse(xml).unwrap();
        let identities: Vec<&str> = project
            .all_sample_refs
            .iter()
            .map(|s| s.identity())
            .collect();
        assert!(identities.contains(&"/clip.wav"));
        assert!(identities.contains(&"/inst.wav"));
        assert_eq!(project.all_sample_refs.len(), 2);
    }

    #[test]
    fn all_sample_refs_dedupes_by_identity() {
        // Same SampleRef appears in two places — should only appear once.
        let xml = r#"<?xml version="1.0"?>
<Ableton><LiveSet>
  <Tracks>
    <AudioTrack Id="1">
      <Name><EffectiveName Value="t"/></Name>
      <DeviceChain><MainSequencer><Sample><ArrangerAutomation><Events>
        <AudioClip Id="1" Time="0">
          <CurrentStart Value="0"/><CurrentEnd Value="4"/>
          <SampleRef><FileRef><Path Value="/dup.wav"/><Name Value="dup.wav"/></FileRef></SampleRef>
        </AudioClip>
        <AudioClip Id="2" Time="8">
          <CurrentStart Value="8"/><CurrentEnd Value="12"/>
          <SampleRef><FileRef><Path Value="/dup.wav"/><Name Value="dup.wav"/></FileRef></SampleRef>
        </AudioClip>
      </Events></ArrangerAutomation></Sample></MainSequencer></DeviceChain>
    </AudioTrack>
  </Tracks>
  <MasterTrack><DeviceChain><Mixer><Tempo><Manual Value="120"/></Tempo></Mixer></DeviceChain></MasterTrack>
</LiveSet></Ableton>"#;
        let project = parse(xml).unwrap();
        assert_eq!(project.all_sample_refs.len(), 1);
        assert_eq!(project.all_sample_refs[0].identity(), "/dup.wav");
    }

    #[test]
    fn all_sample_refs_skips_node_with_no_file_ref() {
        // A bare <SampleRef/> with no FileRef child should be silently skipped.
        let xml = r#"<?xml version="1.0"?>
<Ableton><LiveSet>
  <Tracks>
    <AudioTrack Id="1">
      <Name><EffectiveName Value="t"/></Name>
      <DeviceChain><MainSequencer><Sample><ArrangerAutomation><Events>
        <AudioClip Id="1" Time="0">
          <CurrentStart Value="0"/><CurrentEnd Value="4"/>
          <SampleRef><FileRef><Path Value="/ok.wav"/><Name Value="ok.wav"/></FileRef></SampleRef>
        </AudioClip>
      </Events></ArrangerAutomation></Sample></MainSequencer></DeviceChain>
    </AudioTrack>
    <MidiTrack Id="2">
      <DeviceChain><Devices><Sampler><SampleRef/></Sampler></Devices></DeviceChain>
    </MidiTrack>
  </Tracks>
  <MasterTrack><DeviceChain><Mixer><Tempo><Manual Value="120"/></Tempo></Mixer></DeviceChain></MasterTrack>
</LiveSet></Ableton>"#;
        let project = parse(xml).unwrap();
        assert_eq!(project.all_sample_refs.len(), 1);
        assert_eq!(project.all_sample_refs[0].identity(), "/ok.wav");
    }

    #[test]
    fn falls_back_to_path_basename_when_name_missing() {
        let xml = r#"<?xml version="1.0"?>
<Ableton><LiveSet>
  <Tracks>
    <AudioTrack Id="1">
      <Name><EffectiveName Value="t"/></Name>
      <DeviceChain><MainSequencer><Sample><ArrangerAutomation><Events>
        <AudioClip Id="1" Time="0">
          <CurrentStart Value="0"/>
          <CurrentEnd Value="4"/>
          <SampleRef>
            <FileRef>
              <Path Value="C:/proj/sub/named.wav"/>
            </FileRef>
          </SampleRef>
        </AudioClip>
      </Events></ArrangerAutomation></Sample></MainSequencer></DeviceChain>
    </AudioTrack>
  </Tracks>
  <MasterTrack><DeviceChain><Mixer><Tempo><Manual Value="120"/></Tempo></Mixer></DeviceChain></MasterTrack>
</LiveSet></Ableton>"#;
        let project = parse(xml).unwrap();
        let clip = &project.audio_tracks[0].clips[0];
        assert_eq!(clip.sample.name, "named.wav");
    }
}

use navaq_core::{
    AudioSource, AvaCore, AvaExecutionRoute, AvaIntent, AvaSettings, PlaybackStatus, PlayerCommand,
    PlayerCore, Track, TrackId, UiSettings,
};

fn track(id: &str) -> Track {
    Track {
        id: TrackId::from(id),
        title: format!("Track {id}"),
        artist: Some("NAVAQ Test".into()),
        album: Some("Core Contracts".into()),
        duration_ms: Some(180_000),
        source: AudioSource::LocalFile(format!("/music/{id}.flac")),
    }
}

#[test]
fn public_playback_contract_survives_snapshot_round_trip() {
    let mut player = PlayerCore::new();
    let queued = player
        .handle(PlayerCommand::AddToQueue(track("contract-1")))
        .expect("valid track should enter the queue");
    assert!(!queued.backend_actions.is_empty());

    player
        .handle(PlayerCommand::Play)
        .expect("queued track should start");
    assert_eq!(player.state().status, PlaybackStatus::Playing);

    let encoded = serde_json::to_vec(&player.snapshot()).expect("snapshot serializes");
    let snapshot = serde_json::from_slice(&encoded).expect("snapshot deserializes");
    let (restored, output) = PlayerCore::restore(snapshot).expect("snapshot restores");

    assert_eq!(restored.state().status, PlaybackStatus::Paused);
    assert_eq!(
        restored.current_track_id(),
        Some(&TrackId::from("contract-1"))
    );
    assert!(output.backend_actions.iter().any(|action| {
        matches!(action, navaq_core::BackendAction::Prepare(prepared)
            if prepared.id == TrackId::from("contract-1"))
    }));
}

#[test]
fn public_ava_contract_is_local_and_bilingual() {
    let ava = AvaCore::new();
    let settings = AvaSettings::default();

    let persian = ava.interpret("هی آوا، آهنگ بعدی", &settings, false);
    assert_eq!(persian.route, AvaExecutionRoute::Local);
    assert_eq!(
        persian.intent,
        AvaIntent::Transport {
            command: PlayerCommand::Next
        }
    );

    let english = ava.interpret("Hey Ava, pause", &settings, false);
    assert_eq!(english.route, AvaExecutionRoute::Local);
    assert_eq!(
        english.intent,
        AvaIntent::Transport {
            command: PlayerCommand::Pause
        }
    );
}

#[test]
fn public_ui_settings_contract_is_json_stable_and_validated() {
    let settings = UiSettings::default();
    settings.validate().expect("default settings are valid");

    let json = serde_json::to_string(&settings).expect("settings serialize");
    let decoded: UiSettings = serde_json::from_str(&json).expect("settings deserialize");
    assert_eq!(decoded, settings);

    let repaired = UiSettings {
        accent: "not-a-color".into(),
        blur_px: u8::MAX,
        ..settings
    }
    .sanitized();
    repaired.validate().expect("sanitized settings are valid");
}

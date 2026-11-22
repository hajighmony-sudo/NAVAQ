use crate::audio_pipeline::{PcmAudioPipeline, PcmFormat, PcmPipelineSettings};
use crate::ava::{AvaCore, AvaSettings};
use crate::creative::LyricToMelodyRequest;
use crate::downloads::validate_http_url;
use crate::equalizer::EqualizerSettings;
use crate::haptic_analyzer::HapticAnalyzer;
use crate::melody::generate_from_request;
use crate::spatial::SpatialSettings;
use crate::spatial_render::render as render_room;
use crate::{BackendEvent, PlayerCommand, PlayerCore};
use jni::objects::{JClass, JFloatArray, JString};
use jni::sys::{jboolean, jlong, jstring};
use jni::JNIEnv;
use std::ptr;
use std::sync::Mutex;

fn json_string<'local>(env: &mut JNIEnv<'local>, value: String) -> jstring {
    match env.new_string(value) {
        Ok(value) => value.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeCreate(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    Box::into_raw(Box::new(PlayerCore::new())) as jlong
}

#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeHandleCommand(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    command_json: JString,
) -> jstring {
    if handle == 0 {
        return json_string(&mut env, r#"{"error":"invalid_core_handle"}"#.to_owned());
    }

    let input: String = match env.get_string(&command_json) {
        Ok(value) => value.into(),
        Err(_) => return json_string(&mut env, r#"{"error":"invalid_command_string"}"#.to_owned()),
    };

    let command = match serde_json::from_str::<PlayerCommand>(&input) {
        Ok(command) => command,
        Err(error) => {
            return json_string(
                &mut env,
                serde_json::json!({"error":"invalid_command","message":error.to_string()})
                    .to_string(),
            );
        }
    };

    let core = unsafe { &mut *(handle as *mut PlayerCore) };
    let response = match core.handle(command) {
        Ok(output) => serde_json::to_string(&output)
            .unwrap_or_else(|error| serde_json::json!({"error":error.to_string()}).to_string()),
        Err(error) => serde_json::json!({"error":error.to_string()}).to_string(),
    };
    json_string(&mut env, response)
}

#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeHandleBackendEvent(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    event_json: JString,
) -> jstring {
    if handle == 0 {
        return json_string(&mut env, r#"{"error":"invalid_core_handle"}"#.to_owned());
    }

    let input: String = match env.get_string(&event_json) {
        Ok(value) => value.into(),
        Err(_) => return json_string(&mut env, r#"{"error":"invalid_event_string"}"#.to_owned()),
    };
    let event = match serde_json::from_str::<BackendEvent>(&input) {
        Ok(event) => event,
        Err(error) => {
            return json_string(
                &mut env,
                serde_json::json!({"error":"invalid_event","message":error.to_string()})
                    .to_string(),
            );
        }
    };

    let core = unsafe { &mut *(handle as *mut PlayerCore) };
    let output = core.handle_backend_event(event);
    let response = serde_json::to_string(&output)
        .unwrap_or_else(|error| serde_json::json!({"error":error.to_string()}).to_string());
    json_string(&mut env, response)
}

#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeDestroy(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle != 0 {
        unsafe {
            drop(Box::from_raw(handle as *mut PlayerCore));
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeInterpretAva(
    mut env: JNIEnv,
    _class: JClass,
    transcript: JString,
    settings_json: JString,
    network_available: jboolean,
) -> jstring {
    let transcript: String = match env.get_string(&transcript) {
        Ok(value) => value.into(),
        Err(_) => return json_string(&mut env, r#"{"error":"invalid_transcript"}"#.to_owned()),
    };
    let settings_input: String = match env.get_string(&settings_json) {
        Ok(value) => value.into(),
        Err(_) => return json_string(&mut env, r#"{"error":"invalid_settings"}"#.to_owned()),
    };
    let settings = serde_json::from_str::<AvaSettings>(&settings_input).unwrap_or_default();
    let decision = AvaCore::new().interpret(&transcript, &settings, network_available != 0);
    let response = serde_json::to_string(&decision)
        .unwrap_or_else(|error| serde_json::json!({"error":error.to_string()}).to_string());
    json_string(&mut env, response)
}

#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeValidateDownloadUrl(
    mut env: JNIEnv,
    _class: JClass,
    url: JString,
) -> jboolean {
    let input: String = match env.get_string(&url) {
        Ok(value) => value.into(),
        Err(_) => return 0,
    };
    if validate_http_url(&input).is_ok() {
        1
    } else {
        0
    }
}

type SharedPcmPipeline = Mutex<PcmAudioPipeline>;

#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeCreateDsp(
    _env: JNIEnv,
    _class: JClass,
    sample_rate: jni::sys::jint,
    channels: jni::sys::jint,
) -> jlong {
    if !(1..=384_000).contains(&sample_rate) || !(1..=8).contains(&channels) {
        return 0;
    }
    let format = PcmFormat {
        sample_rate: sample_rate as u32,
        channels: channels as u16,
    };
    match PcmAudioPipeline::new(format, PcmPipelineSettings::default()) {
        Ok(pipeline) => Box::into_raw(Box::new(Mutex::new(pipeline))) as jlong,
        Err(_) => 0,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeProcessDsp(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    samples: JFloatArray,
) -> jboolean {
    if handle == 0 {
        return 0;
    }
    let length = match env.get_array_length(&samples) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let mut values = vec![0.0_f32; length as usize];
    if env
        .get_float_array_region(&samples, 0, &mut values)
        .is_err()
    {
        return 0;
    }
    let shared = unsafe { &*(handle as *const SharedPcmPipeline) };
    let Ok(mut pipeline) = shared.lock() else {
        return 0;
    };
    if pipeline.process_interleaved(&mut values).is_err() {
        return 0;
    }
    if env.set_float_array_region(&samples, 0, &values).is_err() {
        return 0;
    }
    1
}

#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeSetDspEqualizer(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    equalizer_json: JString,
) -> jboolean {
    if handle == 0 {
        return 0;
    }
    let input: String = match env.get_string(&equalizer_json) {
        Ok(v) => v.into(),
        Err(_) => return 0,
    };
    let equalizer = match serde_json::from_str::<EqualizerSettings>(&input) {
        Ok(v) if v.is_valid() => v,
        _ => return 0,
    };
    let shared = unsafe { &*(handle as *const SharedPcmPipeline) };
    let Ok(mut pipeline) = shared.lock() else {
        return 0;
    };
    let mut settings = pipeline.settings().clone();
    settings.equalizer = equalizer;
    if pipeline.set_settings(settings).is_ok() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeDestroyDsp(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle != 0 {
        unsafe {
            drop(Box::from_raw(handle as *mut SharedPcmPipeline));
        }
    }
}

/// Applies a simplified room reverb (single echo tap) to the PCM pipeline.
/// `mix <= 0` or `delay_ms == 0` removes it.
#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeSetDspRoom(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    delay_ms: jni::sys::jint,
    feedback: jni::sys::jfloat,
    mix: jni::sys::jfloat,
) -> jboolean {
    if handle == 0 {
        return 0;
    }
    let shared = unsafe { &*(handle as *const SharedPcmPipeline) };
    let Ok(mut pipeline) = shared.lock() else {
        return 0;
    };
    let mut settings = pipeline.settings().clone();
    settings.echo = if delay_ms <= 0 || mix <= 0.0 {
        None
    } else {
        Some(crate::audio_pipeline::EchoSettings {
            delay_ms: delay_ms as u32,
            feedback,
            mix,
        })
    };
    if pipeline.set_settings(settings).is_ok() {
        1
    } else {
        0
    }
}

/// Renders a personalised listening room (`SpatialSettings` JSON) to speaker feeds,
/// first reflections and Sabine RT60 as JSON. Errors are `{"error": "..."}`.
#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeRenderSpatial(
    mut env: JNIEnv,
    _class: JClass,
    settings_json: JString,
) -> jstring {
    let input: String = match env.get_string(&settings_json) {
        Ok(v) => v.into(),
        Err(_) => return json_string(&mut env, r#"{"error":"invalid_string"}"#.to_owned()),
    };
    let response = match serde_json::from_str::<SpatialSettings>(&input) {
        Err(error) => serde_json::json!({"error": error.to_string()}).to_string(),
        Ok(settings) => match render_room(&settings) {
            Err(error) => serde_json::json!({"error": error.to_string()}).to_string(),
            Ok(r) => serde_json::json!({
                "rt60_s": r.rt60_s,
                "critical_distance_m": r.critical_distance_m,
                "absorption_sabins": r.absorption_sabins,
                "reverb_mix": r.reverb_mix,
                "speakers": r.speakers.iter().map(|f| serde_json::json!({
                    "name": f.name,
                    "azimuth_deg": f.azimuth_deg,
                    "distance_m": f.distance_m,
                    "delay_ms": f.delay_ms,
                    "alignment_delay_ms": f.alignment_delay_ms,
                    "gain": f.gain,
                    "compensation_gain": f.compensation_gain,
                    "itd_ms": f.itd_ms,
                })).collect::<Vec<_>>(),
                "reflections": r.reflections.iter().map(|x| serde_json::json!({
                    "speaker_index": x.speaker_index,
                    "surface": format!("{:?}", x.surface),
                    "delay_ms": x.delay_ms,
                    "gain": x.gain,
                })).collect::<Vec<_>>(),
            })
            .to_string(),
        },
    };
    json_string(&mut env, response)
}

/// Turns lyrics into a melody + harmony song and writes `<out_base>.wav` and
/// `<out_base>.mid`. The request is a `LyricToMelodyRequest` JSON.
#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeGenerateSong(
    mut env: JNIEnv,
    _class: JClass,
    request_json: JString,
    out_base: JString,
) -> jstring {
    let request: String = match env.get_string(&request_json) {
        Ok(v) => v.into(),
        Err(_) => return json_string(&mut env, r#"{"error":"invalid_request"}"#.to_owned()),
    };
    let base: String = match env.get_string(&out_base) {
        Ok(v) => v.into(),
        Err(_) => return json_string(&mut env, r#"{"error":"invalid_path"}"#.to_owned()),
    };
    let response = (|| -> Result<String, String> {
        let request: LyricToMelodyRequest =
            serde_json::from_str(&request).map_err(|e| e.to_string())?;
        let song = generate_from_request(&request).map_err(|e| e.to_string())?;
        let wav = song.to_wav_bytes(44_100).map_err(|e| e.to_string())?;
        let midi = song.to_midi_bytes().map_err(|e| e.to_string())?;
        let wav_path = format!("{base}.wav");
        let midi_path = format!("{base}.mid");
        std::fs::write(&wav_path, wav).map_err(|e| e.to_string())?;
        std::fs::write(&midi_path, midi).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "ok": true,
            "title": song.title,
            "duration_ms": song.duration_ms(),
            "wav": wav_path,
            "midi": midi_path,
        })
        .to_string())
    })()
    .unwrap_or_else(|error| serde_json::json!({"error": error}).to_string());
    json_string(&mut env, response)
}

type SharedHapticAnalyzer = Mutex<HapticAnalyzer>;

#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeCreateHaptic(
    _env: JNIEnv,
    _class: JClass,
    sample_rate: jni::sys::jint,
    channels: jni::sys::jint,
) -> jlong {
    if !(1..=384_000).contains(&sample_rate) || !(1..=8).contains(&channels) {
        return 0;
    }
    match HapticAnalyzer::new(sample_rate as u32, channels as usize) {
        Some(analyzer) => Box::into_raw(Box::new(Mutex::new(analyzer))) as jlong,
        None => 0,
    }
}

/// Feeds interleaved PCM to the analyzer and returns detected events as a JSON array of
/// `{time_ms, trigger, intensity}` (trigger is `bass_hit`, `snare` or `hi_hat`).
#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeProcessHaptic(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    samples: JFloatArray,
) -> jstring {
    if handle == 0 {
        return json_string(&mut env, "[]".to_owned());
    }
    let length = match env.get_array_length(&samples) {
        Ok(v) => v,
        Err(_) => return json_string(&mut env, "[]".to_owned()),
    };
    let mut values = vec![0.0_f32; length as usize];
    if env
        .get_float_array_region(&samples, 0, &mut values)
        .is_err()
    {
        return json_string(&mut env, "[]".to_owned());
    }
    let shared = unsafe { &*(handle as *const SharedHapticAnalyzer) };
    let Ok(mut analyzer) = shared.lock() else {
        return json_string(&mut env, "[]".to_owned());
    };
    let events: Vec<serde_json::Value> = analyzer
        .process(&values)
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "time_ms": e.time_ms,
                "trigger": serde_json::to_value(e.trigger).unwrap_or_default(),
                "intensity": e.intensity,
            })
        })
        .collect();
    json_string(&mut env, serde_json::Value::Array(events).to_string())
}

#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeDestroyHaptic(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle != 0 {
        unsafe {
            drop(Box::from_raw(handle as *mut SharedHapticAnalyzer));
        }
    }
}

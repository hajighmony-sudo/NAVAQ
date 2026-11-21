use crate::audio_pipeline::{PcmAudioPipeline, PcmFormat, PcmPipelineSettings};
use crate::equalizer::EqualizerSettings;
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

type SharedPcmPipeline = Mutex<PcmAudioPipeline>;

#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeCreateDsp(
    _env: JNIEnv, _class: JClass, sample_rate: jni::sys::jint, channels: jni::sys::jint,
) -> jlong {
    if sample_rate <= 0 || channels <= 0 { return 0; }
    let format = PcmFormat { sample_rate: sample_rate as u32, channels: channels as u16 };
    match PcmAudioPipeline::new(format, PcmPipelineSettings::default()) {
        Ok(pipeline) => Box::into_raw(Box::new(Mutex::new(pipeline))) as jlong,
        Err(_) => 0,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeProcessDsp(
    mut env: JNIEnv, _class: JClass, handle: jlong, samples: JFloatArray,
) -> jboolean {
    if handle == 0 { return 0; }
    let length = match env.get_array_length(&samples) { Ok(v) => v, Err(_) => return 0 };
    let mut values = vec![0.0_f32; length as usize];
    if env.get_float_array_region(&samples, 0, &mut values).is_err() { return 0; }
    let shared = unsafe { &*(handle as *const SharedPcmPipeline) };
    let Ok(mut pipeline) = shared.lock() else { return 0; };
    if pipeline.process_interleaved(&mut values).is_err() { return 0; }
    if env.set_float_array_region(&samples, 0, &values).is_err() { return 0; }
    1
}

#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeSetDspEqualizer(
    mut env: JNIEnv, _class: JClass, handle: jlong, equalizer_json: JString,
) -> jboolean {
    if handle == 0 { return 0; }
    let input: String = match env.get_string(&equalizer_json) { Ok(v) => v.into(), Err(_) => return 0 };
    let equalizer = match serde_json::from_str::<EqualizerSettings>(&input) {
        Ok(v) if v.is_valid() => v, _ => return 0,
    };
    let shared = unsafe { &*(handle as *const SharedPcmPipeline) };
    let Ok(mut pipeline) = shared.lock() else { return 0; };
    let mut settings = pipeline.settings().clone();
    settings.equalizer = equalizer;
    if pipeline.set_settings(settings).is_ok() { 1 } else { 0 }
}

#[no_mangle]
pub extern "system" fn Java_com_navaq_android_core_NavaqNativeBridge_nativeDestroyDsp(
    _env: JNIEnv, _class: JClass, handle: jlong,
) {
    if handle != 0 { unsafe { drop(Box::from_raw(handle as *mut SharedPcmPipeline)); } }
}

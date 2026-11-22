package com.navaq.android.core;

/** JNI boundary to the Rust playback-domain core and streaming PCM DSP. */
public final class NavaqNativeBridge {
    static {
        System.loadLibrary("navaq_core");
    }

    private NavaqNativeBridge() {}

    public static native long nativeCreate();
    public static native String nativeHandleCommand(long handle, String commandJson);
    public static native String nativeHandleBackendEvent(long handle, String eventJson);
    public static native void nativeDestroy(long handle);

    /** Validate direct HTTP(S) download URLs with the Rust core policy. */
    public static native boolean nativeValidateDownloadUrl(String url);

    /** Interpret a bilingual AVA transcript using the Rust domain parser. */
    public static native String nativeInterpretAva(
            String transcript,
            String settingsJson,
            boolean networkAvailable
    );

    public static native long nativeCreateDsp(int sampleRate, int channels);
    public static native boolean nativeProcessDsp(long handle, float[] samples);
    public static native boolean nativeSetDspEqualizer(long handle, String equalizerJson);
    public static native void nativeDestroyDsp(long handle);

    /** Applies a simplified room reverb to the DSP; mix <= 0 removes it. */
    public static native boolean nativeSetDspRoom(long handle, int delayMs, float feedback, float mix);

    /** Renders SpatialSettings JSON to speaker feeds, reflections and RT60 (JSON). */
    public static native String nativeRenderSpatial(String settingsJson);

    /** Writes a lyric-to-melody song as {outBase}.wav and {outBase}.mid; returns JSON. */
    public static native String nativeGenerateSong(String requestJson, String outBase);

    public static native long nativeCreateHaptic(int sampleRate, int channels);
    public static native String nativeProcessHaptic(long handle, float[] samples);
    public static native void nativeDestroyHaptic(long handle);
}

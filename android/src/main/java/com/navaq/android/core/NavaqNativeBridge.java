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

    public static native long nativeCreateDsp(int sampleRate, int channels);
    public static native boolean nativeProcessDsp(long handle, float[] samples);
    public static native boolean nativeSetDspEqualizer(long handle, String equalizerJson);
    public static native void nativeDestroyDsp(long handle);
}

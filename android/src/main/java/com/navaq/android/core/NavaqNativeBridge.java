package com.navaq.android.core;

/** JNI boundary to the Rust playback-domain core. */
public final class NavaqNativeBridge {
    static {
        System.loadLibrary("navaq_core");
    }

    private NavaqNativeBridge() {}

    public static native long nativeCreate();
    public static native String nativeHandleCommand(long handle, String commandJson);
    public static native String nativeHandleBackendEvent(long handle, String eventJson);
    public static native void nativeDestroy(long handle);
}

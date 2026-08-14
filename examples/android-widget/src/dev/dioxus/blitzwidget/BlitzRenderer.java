package dev.dioxus.blitzwidget;

import android.content.Context;

/**
 * JNI bridge to the Rust-owned widget (blitz-widget-ffi). Rust holds all
 * state (counters, slider, animation clock, playback), handles every action,
 * and decides what is rendered where; Java only shuffles frames and events
 * back and forth.
 */
public final class BlitzRenderer {
    static {
        System.loadLibrary("blitz_widget_ffi");
    }

    private BlitzRenderer() {}

    /** Path of the key=value file where Rust persists all widget state. */
    public static String statePath(Context context) {
        return context.getFilesDir().getAbsolutePath() + "/blitz-widget-state.txt";
    }

    /** Applies a tapped element's data-action to the Rust-owned state. */
    public static native void dispatch(String statePath, String action);

    /**
     * RGBA8888 pixels of (width*scale) x (height*scale) for one frame of a
     * widget kind ("counter", "counter-lock", "interactive", "anim") at the
     * current Rust-owned state, with CSS animations sampled at timeSecs.
     * Suitable for Bitmap.copyPixelsFromBuffer on an ARGB_8888 bitmap.
     * clock is a display-only time string (used by "counter").
     */
    public static native byte[] renderWidget(
            String statePath, String kind, int width, int height, float scale,
            double timeSecs, String clock);

    /**
     * The JSON compositing plan of a widget kind's frame at the current
     * Rust-owned state:
     * {"buttons":[{"action":..,"x":..,"y":..,"width":..,"height":..},..],
     *  "layers":[..]} (coordinates in CSS px / dp).
     */
    public static native String widgetPlan(
            String statePath, String kind, int width, int height, float scale);

    /** The animation widget's current CSS animation clock, in seconds. */
    public static native double animTime(String statePath);

    /** The animation clock {@code elapsed} seconds into playback (wraps). */
    public static native double animTimeAt(String statePath, double elapsed);

    /** Seconds of flip-book playback triggered by the "play" action. */
    public static native double playSecs();
}

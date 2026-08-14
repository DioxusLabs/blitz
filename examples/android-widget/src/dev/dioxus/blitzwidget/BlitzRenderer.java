package dev.dioxus.blitzwidget;

import android.content.Context;

/**
 * JNI bridge to the Blitz HTML/CSS renderer (blitz-widget-ffi). All widget
 * state (counters, slider, animation clock, playback) is owned and persisted
 * by Rust; Java only forwards tapped data-actions and blits rendered frames.
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

    /**
     * Renders HTML to RGBA8888 pixels of (width*scale) x (height*scale).
     * Suitable for Bitmap.copyPixelsFromBuffer on an ARGB_8888 bitmap.
     */
    public static native byte[] renderHtml(String html, int width, int height, float scale);

    /**
     * Returns a JSON array of hit regions
     * ([{"action":..,"x":..,"y":..,"width":..,"height":..}, ..], coordinates
     * in CSS px / dp) for all elements with a data-action attribute.
     */
    public static native String extractRegions(String html, int width, int height, float scale);

    /**
     * Like renderHtml, but samples CSS animations/transitions at timeSecs on
     * the document's animation clock (animations start at t=0).
     */
    public static native byte[] renderHtmlAt(
            String html, int width, int height, float scale, double timeSecs);

    /** Applies a tapped element's data-action to the Rust-owned state. */
    public static native void dispatch(String statePath, String action);

    /**
     * HTML for a widget kind ("counter", "counter-lock", "interactive",
     * "anim") at the current Rust-owned state. clock is a display-only time
     * string (used by "counter").
     */
    public static native String widgetHtml(
            String statePath, String kind, boolean hideTracked, String clock);

    /** The animation widget's current CSS animation clock, in seconds. */
    public static native double animTime(String statePath);

    /** The animation clock {@code elapsed} seconds into playback (wraps). */
    public static native double animTimeAt(String statePath, double elapsed);

    /** Seconds of flip-book playback triggered by the "play" action. */
    public static native double playSecs();
}

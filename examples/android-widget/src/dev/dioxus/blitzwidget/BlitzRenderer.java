package dev.dioxus.blitzwidget;

/** JNI bridge to the Blitz HTML/CSS renderer (blitz-widget-ffi). */
public final class BlitzRenderer {
    static {
        System.loadLibrary("blitz_widget_ffi");
    }

    private BlitzRenderer() {}

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

    /** Builds the demo widget HTML (counter + slider) for the given state. */
    public static native String demoWidgetHtml(int count, int slider);

    /**
     * Like renderHtml, but samples CSS animations/transitions at timeSecs on
     * the document's animation clock (animations start at t=0).
     */
    public static native byte[] renderHtmlAt(
            String html, int width, int height, float scale, double timeSecs);

    /**
     * Builds the animated demo widget HTML (CSS keyframes + time scrubber).
     * scrub is the highlighted scrubber segment (0..=10).
     */
    public static native String demoAnimatedHtml(int scrub, boolean hideTracked);
}

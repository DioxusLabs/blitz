#ifndef BLITZ_WIDGET_H
#define BLITZ_WIDGET_H

#include <stdint.h>
#include <stddef.h>

// Render NUL-terminated UTF-8 HTML to an RGBA8888 buffer of
// (width*scale) x (height*scale) physical pixels.
// Returns a heap buffer of *out_len bytes; free with blitz_buffer_free.
// Returns NULL on invalid input.
uint8_t *blitz_render_html(const char *html, uint32_t width, uint32_t height,
                           float scale, size_t *out_len);

// Like blitz_render_html, but also writes a JSON array of hit regions
// ([{"action":..,"x":..,"y":..,"width":..,"height":..}, ..], coordinates in
// points) for all elements with a data-action attribute to *out_regions_json.
// Free the JSON with blitz_string_free.
// time_secs samples CSS animations/transitions at that instant of the
// document's animation clock (animations start at t=0).
uint8_t *blitz_render_html_with_regions(const char *html, uint32_t width,
                                        uint32_t height, float scale,
                                        double time_secs, size_t *out_len,
                                        char **out_regions_json);

// Like blitz_render_html, but samples CSS animations at time_secs.
uint8_t *blitz_render_html_at(const char *html, uint32_t width,
                              uint32_t height, float scale, double time_secs,
                              size_t *out_len);

// Rust-owned widget state. All state (counters, slider, animation clock,
// playback) is persisted by Rust at state_path; the native shell only
// forwards tapped data-actions and blits the returned frames.

// Apply a data-action from a tapped widget element to the persisted state.
void blitz_widget_dispatch(const char *state_path, const char *action);

// HTML for a widget kind ("counter", "counter-lock", "interactive", "anim")
// at the current state. clock is a display-only time string (used by
// "counter"; may be NULL). With hide_tracked != 0 the data-track elements
// keep their layout but don't paint, for native layer compositing.
// Free with blitz_string_free; NULL for an unknown kind.
char *blitz_widget_html(const char *state_path, const char *kind,
                        int32_t hide_tracked, const char *clock);

// The animation widget's current CSS animation clock, in seconds.
double blitz_widget_anim_time(const char *state_path);

// The animation clock `elapsed` seconds into playback (wraps the cycle).
double blitz_widget_anim_time_at(const char *state_path, double elapsed);

// Seconds of flip-book playback triggered by the "play" action.
double blitz_widget_play_secs(void);

// Plan the animation widget's timeline as JSON:
// {"frames":[{"offset":..,"time":..},..]} — offset in seconds relative to
// now, time the animation clock to render at. One frame normally; a
// flip-book right after a "play" dispatch (consuming clears the pending
// playback). Free with blitz_string_free.
char *blitz_widget_anim_timeline_json(const char *state_path);

// Standalone sprites (ball / progress fill) for native layer compositing.
// Free with blitz_string_free.
char *blitz_demo_ball_sprite_html(void);
char *blitz_demo_fill_sprite_html(void);

void blitz_buffer_free(uint8_t *ptr, size_t len);
void blitz_string_free(char *ptr);

#endif

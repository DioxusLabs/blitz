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

// Build the demo widget HTML (counter + slider) for the given state.
// Free with blitz_string_free.
char *blitz_demo_widget_html(int32_t count, int32_t slider);

// Build the animated demo widget HTML (CSS keyframes + time scrubber).
// scrub is the highlighted scrubber segment (0..=10). With hide_tracked != 0
// the data-track elements keep their layout but don't paint, for native
// layer compositing. Free with blitz_string_free.
char *blitz_demo_animated_html(int32_t scrub, int32_t hide_tracked);

// Standalone sprites (ball / progress fill) for native layer compositing.
// Free with blitz_string_free.
char *blitz_demo_ball_sprite_html(void);
char *blitz_demo_fill_sprite_html(void);

void blitz_buffer_free(uint8_t *ptr, size_t len);
void blitz_string_free(char *ptr);

#endif

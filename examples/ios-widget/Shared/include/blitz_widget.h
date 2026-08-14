#ifndef BLITZ_WIDGET_H
#define BLITZ_WIDGET_H

#include <stdint.h>
#include <stddef.h>

// Rust-owned widget. All state (counters, slider, animation clock, playback)
// is persisted by Rust at state_path; Rust handles every action and decides
// what is rendered where. The native shell only forwards tapped data-actions
// and composites the returned frames per the plan.

// Apply a data-action from a tapped widget element to the persisted state.
void blitz_widget_dispatch(const char *state_path, const char *action);

// Render one complete frame of a widget kind ("counter", "counter-lock",
// "interactive", "anim") at the current state: returns the background
// RGBA8888 buffer of *out_len bytes ((width*scale) x (height*scale) pixels;
// free with blitz_buffer_free) and writes the JSON compositing plan
// {"buttons":[{"action","x","y","width","height"},..],
//  "layers":[{"track","x","y","width","height",
//             "spriteWidth","spriteHeight","clipWidth"},..]}
// (coordinates in points; free with blitz_string_free) to *out_plan_json.
// time_secs samples CSS animations at that instant of the animation clock.
// clock is a display-only time string (used by "counter"; may be NULL).
// With hide_tracked != 0 the data-track elements keep their layout but don't
// paint, and the plan includes their sprite layers for native compositing.
// Returns NULL for an unknown kind.
uint8_t *blitz_widget_frame(const char *state_path, const char *kind,
                            uint32_t width, uint32_t height, float scale,
                            double time_secs, int32_t hide_tracked,
                            const char *clock, size_t *out_len,
                            char **out_plan_json);

// Render the standalone sprite of a data-track layer (from the frame plan)
// to an RGBA8888 buffer of *out_len bytes at (width*scale) x (height*scale)
// pixels. Free with blitz_buffer_free; NULL for an unknown track.
uint8_t *blitz_widget_sprite(const char *track, uint32_t width,
                             uint32_t height, float scale, size_t *out_len);

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

void blitz_buffer_free(uint8_t *ptr, size_t len);
void blitz_string_free(char *ptr);

#endif

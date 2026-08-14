package dev.dioxus.blitzwidget;

import android.app.PendingIntent;
import android.appwidget.AppWidgetManager;
import android.appwidget.AppWidgetProvider;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.graphics.Bitmap;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.util.TypedValue;
import android.view.View;
import android.widget.RemoteViews;

import org.json.JSONArray;
import org.json.JSONObject;

import java.nio.ByteBuffer;

/**
 * CSS animation widget: Blitz resolves styles with an explicit animation
 * clock, so every render samples the document's CSS keyframe animations at
 * exactly the instant we choose. The scrubber segments and Step button set
 * that instant; the HTML itself is identical for every frame.
 */
public class BlitzAnimWidgetProvider extends AppWidgetProvider {
    static final String ACTION_ANIM = "dev.dioxus.blitzwidget.ANIM_ACTION";
    static final String EXTRA_ACTION = "blitz_action";
    static final String PREFS = "blitz_anim_widget";
    static final String KEY_TIME_MS = "time_ms";
    static final double DURATION_SECS = 4.0;

    @Override
    public void onUpdate(Context context, AppWidgetManager manager, int[] appWidgetIds) {
        for (int id : appWidgetIds) {
            updateWidget(context, manager, id);
        }
    }

    @Override
    public void onAppWidgetOptionsChanged(
            Context context, AppWidgetManager manager, int appWidgetId, Bundle newOptions) {
        updateWidget(context, manager, appWidgetId);
    }

    @Override
    public void onReceive(Context context, Intent intent) {
        super.onReceive(context, intent);
        if (ACTION_ANIM.equals(intent.getAction())) {
            String action = intent.getStringExtra(EXTRA_ACTION);
            if ("play".equals(action)) {
                play(context);
                return;
            }
            if (action != null) {
                applyAction(context, action);
            }
            AppWidgetManager manager = AppWidgetManager.getInstance(context);
            int[] ids = manager.getAppWidgetIds(
                    new ComponentName(context, BlitzAnimWidgetProvider.class));
            for (int id : ids) {
                updateWidget(context, manager, id);
            }
        }
    }

    /**
     * Flip-book playback: re-render and push successive animation-clock frames
     * for {@code PLAY_MS}. RemoteViews bitmap updates apply immediately, so
     * this animates the widget at the render loop's frame rate. goAsync()
     * keeps the receiver alive for the duration (limit ~10s).
     */
    private void play(Context context) {
        final PendingResult result = goAsync();
        final Context app = context.getApplicationContext();
        new Thread(() -> {
            try {
                AppWidgetManager manager = AppWidgetManager.getInstance(app);
                int[] ids = manager.getAppWidgetIds(
                        new ComponentName(app, BlitzAnimWidgetProvider.class));
                SharedPreferences prefs = app.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
                int baseMs = prefs.getInt(KEY_TIME_MS, 0);
                final long PLAY_MS = 8000;
                final long FRAME_MS = 125;
                long start = System.currentTimeMillis();
                for (long elapsed = 0; elapsed < PLAY_MS;
                        elapsed = System.currentTimeMillis() - start) {
                    double t = ((baseMs + elapsed) % (long) (DURATION_SECS * 1000)) / 1000.0;
                    for (int id : ids) {
                        renderAt(app, manager, id, t);
                    }
                    Thread.sleep(FRAME_MS);
                }
                for (int id : ids) {
                    updateWidget(app, manager, id);
                }
            } catch (Exception ignored) {
            } finally {
                result.finish();
            }
        }).start();
    }

    static void applyAction(Context context, String action) {
        SharedPreferences prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
        int timeMs = prefs.getInt(KEY_TIME_MS, 0);
        if ("step".equals(action)) {
            timeMs = (timeMs + 400) % (int) (DURATION_SECS * 1000);
        } else if (action.startsWith("time:")) {
            try {
                int seg = Integer.parseInt(action.substring(5));
                seg = Math.max(0, Math.min(10, seg));
                timeMs = (int) (seg / 10.0 * DURATION_SECS * 1000);
            } catch (NumberFormatException ignored) {
            }
        }
        prefs.edit().putInt(KEY_TIME_MS, timeMs).apply();
    }

    static void updateWidget(Context context, AppWidgetManager manager, int appWidgetId) {
        SharedPreferences prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
        renderAt(context, manager, appWidgetId, prefs.getInt(KEY_TIME_MS, 0) / 1000.0);
    }

    static void renderAt(
            Context context, AppWidgetManager manager, int appWidgetId, double timeSecs) {
        int scrub = (int) Math.round(timeSecs / DURATION_SECS * 10);

        Bundle options = manager.getAppWidgetOptions(appWidgetId);
        int widthDp = options.getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_WIDTH, 0);
        int heightDp = options.getInt(AppWidgetManager.OPTION_APPWIDGET_MAX_HEIGHT, 0);
        if (widthDp <= 0) widthDp = 250;
        if (heightDp <= 0) heightDp = 120;
        float density = context.getResources().getDisplayMetrics().density;

        String html = BlitzRenderer.demoAnimatedHtml(scrub, false);

        byte[] rgba = BlitzRenderer.renderHtmlAt(html, widthDp, heightDp, density, timeSecs);
        int pw = (int) (widthDp * density);
        int ph = (int) (heightDp * density);
        if (rgba == null || rgba.length != pw * ph * 4) {
            return;
        }
        Bitmap bitmap = Bitmap.createBitmap(pw, ph, Bitmap.Config.ARGB_8888);
        bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(rgba));

        RemoteViews views = new RemoteViews(context.getPackageName(), R.layout.blitz_demo_widget);
        views.setImageViewBitmap(R.id.demo_image, bitmap);

        int used = 0;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            try {
                JSONArray regions = new JSONArray(
                        BlitzRenderer.extractRegions(html, widthDp, heightDp, density));
                for (int i = 0; i < regions.length()
                        && used < BlitzDemoWidgetProvider.HOTSPOT_IDS.length; i++) {
                    JSONObject region = regions.getJSONObject(i);
                    String action = region.getString("action");
                    if (action.startsWith("track:")) continue;
                    int viewId = BlitzDemoWidgetProvider.HOTSPOT_IDS[used++];

                    // Region rects are in CSS px, which equal dp because the
                    // render scale is the display density.
                    views.setViewLayoutMargin(viewId, RemoteViews.MARGIN_LEFT,
                            (float) region.getDouble("x"), TypedValue.COMPLEX_UNIT_DIP);
                    views.setViewLayoutMargin(viewId, RemoteViews.MARGIN_TOP,
                            (float) region.getDouble("y"), TypedValue.COMPLEX_UNIT_DIP);
                    views.setViewLayoutWidth(viewId,
                            (float) region.getDouble("width"), TypedValue.COMPLEX_UNIT_DIP);
                    views.setViewLayoutHeight(viewId,
                            (float) region.getDouble("height"), TypedValue.COMPLEX_UNIT_DIP);
                    views.setViewVisibility(viewId, View.VISIBLE);

                    Intent intent = new Intent(context, BlitzAnimWidgetProvider.class);
                    intent.setAction(ACTION_ANIM);
                    intent.setData(Uri.parse("blitzwidget://anim/" + Uri.encode(action)));
                    intent.putExtra(EXTRA_ACTION, action);
                    PendingIntent pendingIntent = PendingIntent.getBroadcast(
                            context, used, intent,
                            PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
                    views.setOnClickPendingIntent(viewId, pendingIntent);
                }
            } catch (Exception ignored) {
            }
        }
        for (int i = used; i < BlitzDemoWidgetProvider.HOTSPOT_IDS.length; i++) {
            views.setViewVisibility(BlitzDemoWidgetProvider.HOTSPOT_IDS[i], View.GONE);
        }

        manager.updateAppWidget(appWidgetId, views);
    }
}

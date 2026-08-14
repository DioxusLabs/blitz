package dev.dioxus.blitzwidget;

import android.app.PendingIntent;
import android.appwidget.AppWidgetManager;
import android.appwidget.AppWidgetProvider;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
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
 * exactly the instant Rust chooses. The scrubber segments and Step button
 * mutate the Rust-owned clock; the HTML itself is identical for every frame.
 */
public class BlitzAnimWidgetProvider extends AppWidgetProvider {
    static final String ACTION_ANIM = "dev.dioxus.blitzwidget.ANIM_ACTION";
    static final String EXTRA_ACTION = "blitz_action";

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
            if (action != null) {
                BlitzRenderer.dispatch(BlitzRenderer.statePath(context), action);
            }
            if ("play".equals(action)) {
                play(context);
                return;
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
                String statePath = BlitzRenderer.statePath(app);
                final long PLAY_MS = (long) (BlitzRenderer.playSecs() * 1000);
                final long FRAME_MS = 125;
                long start = System.currentTimeMillis();
                for (long elapsed = 0; elapsed < PLAY_MS;
                        elapsed = System.currentTimeMillis() - start) {
                    double t = BlitzRenderer.animTimeAt(statePath, elapsed / 1000.0);
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

    static void updateWidget(Context context, AppWidgetManager manager, int appWidgetId) {
        renderAt(context, manager, appWidgetId,
                BlitzRenderer.animTime(BlitzRenderer.statePath(context)));
    }

    static void renderAt(
            Context context, AppWidgetManager manager, int appWidgetId, double timeSecs) {
        Bundle options = manager.getAppWidgetOptions(appWidgetId);
        int widthDp = options.getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_WIDTH, 0);
        int heightDp = options.getInt(AppWidgetManager.OPTION_APPWIDGET_MAX_HEIGHT, 0);
        if (widthDp <= 0) widthDp = 250;
        if (heightDp <= 0) heightDp = 120;
        float density = context.getResources().getDisplayMetrics().density;

        byte[] rgba = BlitzRenderer.renderWidget(
                BlitzRenderer.statePath(context), "anim", widthDp, heightDp, density,
                timeSecs, "");
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
                JSONArray buttons = new JSONObject(
                        BlitzRenderer.widgetPlan(
                                BlitzRenderer.statePath(context), "anim",
                                widthDp, heightDp, density))
                        .getJSONArray("buttons");
                for (int i = 0; i < buttons.length()
                        && used < BlitzDemoWidgetProvider.HOTSPOT_IDS.length; i++) {
                    JSONObject region = buttons.getJSONObject(i);
                    String action = region.getString("action");
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

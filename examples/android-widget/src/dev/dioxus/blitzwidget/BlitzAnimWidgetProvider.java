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
 * CSS transition widget: every state change eases to its new pose via CSS
 * transitions resolved under an explicit Rust-owned clock, so an
 * interrupting action re-baselines and eases from wherever the widget
 * currently is instead of snapping. This provider just re-renders (at
 * "now") for as long as Rust says the widget is in motion.
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
            animate(context);
        }
    }

    /**
     * Motion loop: while Rust reports the widget is in motion (an in-flight
     * transition or playback), re-render the frame at "now" and push it.
     * RemoteViews bitmap updates apply immediately, so this animates the
     * widget at the render loop's frame rate. goAsync() keeps the receiver
     * alive for the duration (limit ~10s, so the loop is capped just under).
     */
    private void animate(Context context) {
        final PendingResult result = goAsync();
        final Context app = context.getApplicationContext();
        new Thread(() -> {
            try {
                AppWidgetManager manager = AppWidgetManager.getInstance(app);
                int[] ids = manager.getAppWidgetIds(
                        new ComponentName(app, BlitzAnimWidgetProvider.class));
                String statePath = BlitzRenderer.statePath(app);
                final long FRAME_MS = 125;
                final long CAP_MS = 9500;
                long start = System.currentTimeMillis();
                while (System.currentTimeMillis() - start < CAP_MS
                        && BlitzRenderer.refreshSecs(statePath) > 0) {
                    for (int id : ids) {
                        renderAt(app, manager, id, 0);
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
        renderAt(context, manager, appWidgetId, 0);
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

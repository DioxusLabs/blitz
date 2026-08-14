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
 * Interactive widget with per-element tap targets: Blitz renders the HTML to
 * a bitmap and reports the layout rect of every element with a `data-action`
 * attribute; one invisible hotspot view is positioned over each rect
 * (API 31+ RemoteViews layout APIs) with a PendingIntent carrying the action.
 */
public class BlitzDemoWidgetProvider extends AppWidgetProvider {
    static final String ACTION_DEMO = "dev.dioxus.blitzwidget.DEMO_ACTION";
    static final String EXTRA_ACTION = "blitz_action";

    static final int[] HOTSPOT_IDS = {
        R.id.hotspot_0, R.id.hotspot_1, R.id.hotspot_2, R.id.hotspot_3,
        R.id.hotspot_4, R.id.hotspot_5, R.id.hotspot_6, R.id.hotspot_7,
        R.id.hotspot_8, R.id.hotspot_9, R.id.hotspot_10, R.id.hotspot_11,
        R.id.hotspot_12, R.id.hotspot_13, R.id.hotspot_14, R.id.hotspot_15,
    };

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
        if (ACTION_DEMO.equals(intent.getAction())) {
            String action = intent.getStringExtra(EXTRA_ACTION);
            if (action != null) {
                BlitzRenderer.dispatch(BlitzRenderer.statePath(context), action);
            }
            AppWidgetManager manager = AppWidgetManager.getInstance(context);
            int[] ids = manager.getAppWidgetIds(
                    new ComponentName(context, BlitzDemoWidgetProvider.class));
            for (int id : ids) {
                updateWidget(context, manager, id);
            }
        }
    }

    static void updateWidget(Context context, AppWidgetManager manager, int appWidgetId) {
        Bundle options = manager.getAppWidgetOptions(appWidgetId);
        int widthDp = options.getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_WIDTH, 0);
        int heightDp = options.getInt(AppWidgetManager.OPTION_APPWIDGET_MAX_HEIGHT, 0);
        if (widthDp <= 0) widthDp = 250;
        if (heightDp <= 0) heightDp = 120;
        float density = context.getResources().getDisplayMetrics().density;

        byte[] rgba = BlitzRenderer.renderWidget(
                BlitzRenderer.statePath(context), "interactive", widthDp, heightDp, density,
                0.0, "");
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
                                BlitzRenderer.statePath(context), "interactive",
                                widthDp, heightDp, density))
                        .getJSONArray("buttons");
                for (int i = 0; i < buttons.length() && used < HOTSPOT_IDS.length; i++) {
                    JSONObject region = buttons.getJSONObject(i);
                    String action = region.getString("action");
                    int viewId = HOTSPOT_IDS[used++];

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

                    Intent intent = new Intent(context, BlitzDemoWidgetProvider.class);
                    intent.setAction(ACTION_DEMO);
                    intent.setData(Uri.parse("blitzwidget://action/" + Uri.encode(action)));
                    intent.putExtra(EXTRA_ACTION, action);
                    PendingIntent pendingIntent = PendingIntent.getBroadcast(
                            context, used, intent,
                            PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
                    views.setOnClickPendingIntent(viewId, pendingIntent);
                }
            } catch (Exception ignored) {
            }
        }
        for (int i = used; i < HOTSPOT_IDS.length; i++) {
            views.setViewVisibility(HOTSPOT_IDS[i], View.GONE);
        }

        manager.updateAppWidget(appWidgetId, views);
    }
}

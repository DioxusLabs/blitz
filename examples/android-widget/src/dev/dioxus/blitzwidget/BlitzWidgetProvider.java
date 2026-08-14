package dev.dioxus.blitzwidget;

import android.app.PendingIntent;
import android.appwidget.AppWidgetManager;
import android.appwidget.AppWidgetProvider;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.graphics.Bitmap;
import android.os.Bundle;
import android.widget.RemoteViews;

import java.nio.ByteBuffer;
import java.text.SimpleDateFormat;
import java.util.Date;
import java.util.Locale;

/** Home-screen widget whose content is HTML/CSS rendered to a bitmap by Blitz. */
public class BlitzWidgetProvider extends AppWidgetProvider {
    static final String ACTION_INCREMENT = "dev.dioxus.blitzwidget.INCREMENT";
    static final String PREFS = "blitz_widget";
    static final String KEY_COUNT = "count";

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
        if (ACTION_INCREMENT.equals(intent.getAction())) {
            SharedPreferences prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
            prefs.edit().putInt(KEY_COUNT, prefs.getInt(KEY_COUNT, 0) + 1).apply();
            AppWidgetManager manager = AppWidgetManager.getInstance(context);
            int[] ids = manager.getAppWidgetIds(
                    new ComponentName(context, BlitzWidgetProvider.class));
            for (int id : ids) {
                updateWidget(context, manager, id);
            }
        }
    }

    static void updateWidget(Context context, AppWidgetManager manager, int appWidgetId) {
        SharedPreferences prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
        int count = prefs.getInt(KEY_COUNT, 0);

        Bundle options = manager.getAppWidgetOptions(appWidgetId);
        int widthDp = options.getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_WIDTH, 0);
        int heightDp = options.getInt(AppWidgetManager.OPTION_APPWIDGET_MAX_HEIGHT, 0);
        if (widthDp <= 0) widthDp = 250;
        if (heightDp <= 0) heightDp = 120;
        float density = context.getResources().getDisplayMetrics().density;

        String time = new SimpleDateFormat("HH:mm", Locale.US).format(new Date());
        String html = buildHtml(count, time);

        byte[] rgba = BlitzRenderer.renderHtml(html, widthDp, heightDp, density);
        int pw = (int) (widthDp * density);
        int ph = (int) (heightDp * density);
        if (rgba == null || rgba.length != pw * ph * 4) {
            return;
        }
        Bitmap bitmap = Bitmap.createBitmap(pw, ph, Bitmap.Config.ARGB_8888);
        bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(rgba));

        RemoteViews views = new RemoteViews(context.getPackageName(), R.layout.blitz_widget);
        views.setImageViewBitmap(R.id.widget_image, bitmap);

        Intent intent = new Intent(context, BlitzWidgetProvider.class);
        intent.setAction(ACTION_INCREMENT);
        PendingIntent pendingIntent = PendingIntent.getBroadcast(
                context, 0, intent,
                PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
        views.setOnClickPendingIntent(R.id.widget_image, pendingIntent);

        manager.updateAppWidget(appWidgetId, views);
    }

    static String buildHtml(int count, String time) {
        return "<!DOCTYPE html><html><head><style>"
                + "body { margin: 0; font-family: sans-serif; }"
                + ".card { box-sizing: border-box; width: 100%; height: 100vh; padding: 14px;"
                + "  background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);"
                + "  border-radius: 24px; color: white; display: flex; flex-direction: column;"
                + "  justify-content: space-between; }"
                + ".row { display: flex; justify-content: space-between; align-items: center; }"
                + ".title { font-size: 13px; font-weight: 600; opacity: 0.9; }"
                + ".time { font-size: 11px; opacity: 0.7; }"
                + ".count { font-size: 44px; font-weight: bold; text-align: center; }"
                + ".hint { font-size: 11px; text-align: center; opacity: 0.85;"
                + "  background: rgba(255,255,255,0.18); border-radius: 10px; padding: 5px 8px; }"
                + "</style></head><body><div class=\"card\">"
                + "<div class=\"row\"><div class=\"title\">⚡ Blitz Counter</div>"
                + "<div class=\"time\">" + time + "</div></div>"
                + "<div class=\"count\">" + count + "</div>"
                + "<div class=\"hint\">Tap to increment · HTML by Blitz</div>"
                + "</div></body></html>";
    }
}

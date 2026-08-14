package dev.dioxus.blitzwidget;

import android.app.Activity;
import android.os.Bundle;
import android.view.Gravity;
import android.widget.TextView;

public class MainActivity extends Activity {
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        TextView text = new TextView(this);
        text.setText("Blitz Widgets\n\nLong-press the home screen and add the "
                + "\"Blitz Counter\" widget.\n\nThe widget content is HTML/CSS "
                + "rendered by the Blitz engine.");
        text.setGravity(Gravity.CENTER);
        text.setPadding(48, 48, 48, 48);
        setContentView(text);
    }
}

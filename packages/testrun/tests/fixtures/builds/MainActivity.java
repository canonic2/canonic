package com.example.acme.buildfixture;

public final class MainActivity extends android.app.Activity {
    @Override public void onCreate(android.os.Bundle state) {
        super.onCreate(state);
        android.content.SharedPreferences preferences = getSharedPreferences("acme-acceptance", MODE_PRIVATE);
        int count = preferences.getInt("launches", 0) + 1;
        preferences.edit().putInt("launches", count).apply();
        android.widget.TextView view = new android.widget.TextView(this);
        view.setText("Acme " + BuildConfig.API_URL + " launches=" + count);
        setContentView(view);
    }
}

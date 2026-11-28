package com.example.acme.diagnostics;

import android.app.Activity;
import android.os.Bundle;
import android.util.Log;
import android.widget.*;
import java.util.UUID;

/** Local-only diagnostic app. Fatal controls need an explicit arm toggle. */
public final class MainActivity extends Activity {
    private TextView status;
    private Switch corrected, armed;
    private final String birth = UUID.randomUUID().toString();
    @Override public void onCreate(Bundle saved) {
        super.onCreate(saved);
        int launches = getPreferences(0).getInt("launches", 0) + 1;
        getPreferences(0).edit().putInt("launches", launches).commit();
        ScrollView scroll = new ScrollView(this);
        LinearLayout layout = new LinearLayout(this); layout.setOrientation(LinearLayout.VERTICAL);
        layout.setPadding(24, 48, 24, 24); scroll.addView(layout); setContentView(scroll);
        TextView identity = new TextView(this); identity.setText("Acme native Android\nLaunch " + launches + "\nBirth " + birth); layout.addView(identity);
        corrected = new Switch(this); corrected.setId(R.id.acme_corrected); corrected.setText("Corrected mode"); layout.addView(corrected);
        armed = new Switch(this); armed.setId(R.id.acme_armed); armed.setText("Arm app crash / hang / kill"); layout.addView(armed);
        status = new TextView(this); status.setId(R.id.acme_result); status.setText("Acme ready"); layout.addView(status);
        add(layout, "Java exception (caught)", false, () -> AcmeFaults.exception(corrected.isChecked()));
        add(layout, "Kotlin exception (caught)", false, () -> AcmeKotlinFaults.INSTANCE.exception(corrected.isChecked()));
        add(layout, "Retain one MiB", false, () -> Integer.toString(AcmeFaults.retain(corrected.isChecked())));
        add(layout, "Reset", false, () -> { AcmeFaults.reset(); armed.setChecked(false); return "Acme reset"; });
        add(layout, "Java crash", true, () -> AcmeFaults.exception(corrected.isChecked()));
        add(layout, "Kotlin crash", true, () -> AcmeKotlinFaults.INSTANCE.exception(corrected.isChecked()));
        add(layout, "Main-thread hang (6 seconds)", true, () -> { AcmeFaults.mainThreadHang(corrected.isChecked()); return "hang requested"; });
        add(layout, "Background crash", true, () -> { AcmeFaults.backgroundFailure(corrected.isChecked()); return "background requested"; });
        add(layout, "Kill this app", true, () -> { AcmeFaults.killSelf(corrected.isChecked()); return "kill control ready"; });
        Log.i("AcmeDiagnostics", "Acme ready launch=" + launches);
    }
    private interface Action { String run(); }
    private void add(LinearLayout layout, String title, boolean fatal, Action action) {
        Button button = new Button(this); button.setText(title); button.setContentDescription(title); layout.addView(button);
        button.setOnClickListener(view -> {
            if (fatal && !armed.isChecked()) { status.setText("Arm this app first"); return; }
            Log.i("AcmeDiagnostics", "Acme trigger " + title + " corrected=" + corrected.isChecked());
            if (fatal) {
                armed.setChecked(false); status.setText(action.run());
            } else {
                try { status.setText(action.run()); } catch (RuntimeException error) { status.setText(error.toString()); }
            }
            Log.i("AcmeDiagnostics", "Acme result " + status.getText());
        });
    }
    @Override public void onDestroy() { AcmeFaults.reset(); super.onDestroy(); }
}

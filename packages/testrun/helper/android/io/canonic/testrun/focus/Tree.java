package io.canonic.testrun.focus;

import android.app.UiAutomation;
import android.graphics.Rect;
import android.util.Xml;
import android.os.Bundle;
import android.view.accessibility.AccessibilityNodeInfo;
import java.io.StringWriter;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import org.json.JSONObject;
import org.xmlpull.v1.XmlSerializer;

/** Bounded traversal of the exposed active-window root, without waiting for idle. */
final class Tree {
    private final StringWriter output = new StringWriter();
    private final XmlSerializer xml = Xml.newSerializer();
    private int count;
    private int requests;
    private boolean truncated;
    private int wanted;
    private AccessibilityNodeInfo selected;
    private String document;

    static String snapshot(AccessibilityNodeInfo root, UiAutomation automation) throws Exception {
        return observe(root, automation, 0).document;
    }

    private static Tree observe(AccessibilityNodeInfo root, UiAutomation automation, int wanted) throws Exception {
        Tree tree = new Tree();
        tree.wanted = wanted;
        tree.xml.setOutput(tree.output);
        tree.node(root, 0);
        tree.xml.flush();
        AccessibilityNodeInfo current = automation.getRootInActiveWindow();
        try {
            if (current == null || current.getWindowId() != root.getWindowId()) tree.truncated = true;
        } finally { if (current != null) current.recycle(); }
        tree.document = "<hierarchy scope=\"active_window\" window-id=\"" + root.getWindowId() + "\" truncated=\"" + tree.truncated
                + "\">" + tree.output.toString() + "</hierarchy>";
        return tree;
    }

    /** Revalidate a host-resolved target in this connection before one set-text action. */
    static String replace(AccessibilityNodeInfo root, UiAutomation automation, JSONObject request) throws Exception {
        String text = request.getString("text");
        if (text.getBytes(StandardCharsets.UTF_8).length > 65536) throw new IllegalArgumentException();
        Tree tree = observe(root, automation, request.getInt("index"));
        AccessibilityNodeInfo field = tree.selected;
        AccessibilityNodeInfo focused = null;
        try {
            // Query proof compares the complete exposed content, including package,
            // not a window number that may change across automation connections.
            byte[] digest = MessageDigest.getInstance("SHA-256").digest(tree.output.toString().getBytes(StandardCharsets.UTF_8));
            StringBuilder hex = new StringBuilder();
            for (byte value : digest) hex.append(String.format("%02x", value & 255));
            if (tree.truncated || !hex.toString().equals(request.getString("digest")) || field == null)
                return "{\"version\":1,\"code\":\"STALE_ELEMENT\"}";
            focused = root.findFocus(AccessibilityNodeInfo.FOCUS_INPUT);
            if (!field.refresh() || focused == null || !field.equals(focused)
                    || !field.isFocused() || !field.isEditable() || !field.isEnabled()
                    || !field.isVisibleToUser() || field.getWindowId() != root.getWindowId())
                return "{\"version\":1,\"code\":\"FOCUS_NOT_PROVEN\"}";
            if (!field.getActionList().contains(AccessibilityNodeInfo.AccessibilityAction.ACTION_SET_TEXT))
                return "{\"version\":1,\"code\":\"CAPABILITY_UNAVAILABLE\"}";
            Bundle args = new Bundle();
            args.putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text);
            // From this call onward, any exception or lost reply is an unknown mutation outcome.
            Main.mutationDispatched = true;
            boolean completed = field.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args);
            return completed ? "{\"version\":1,\"completed\":true}"
                    : "{\"version\":1,\"code\":\"ACTION_REFUSED\"}";
        } finally {
            if (field != null) field.recycle();
            if (focused != null) focused.recycle();
        }
    }

    private void node(AccessibilityNodeInfo node, int depth) throws Exception {
        if (count >= 3000 || depth >= 60 || output.getBuffer().length() >= 262144) {
            truncated = true;
            return;
        }
        count++;
        if (count == wanted) selected = AccessibilityNodeInfo.obtain(node);
        xml.startTag(null, "node");
        boolean secure = node.isPassword();
        attr("password", Boolean.toString(secure));
        attr("class", node.getClassName());
        attr("package", node.getPackageName());
        attr("resource-id", node.getViewIdResourceName());
        // Never fetch secure field contents or accessibility labels. Editable
        // values keep up to the replacement limit so typed text can be read back.
        if (!secure) {
            text("text", node.getText(), node.isEditable() ? 65536 : 1024);
            text("content-desc", node.getContentDescription(), 1024);
        }
        attr("enabled", Boolean.toString(node.isEnabled()));
        attr("focused", Boolean.toString(node.isFocused()));
        attr("clickable", Boolean.toString(node.isClickable()));
        attr("selected", Boolean.toString(node.isSelected()));
        attr("checked", Boolean.toString(node.isChecked()));
        attr("checkable", Boolean.toString(node.isCheckable()));
        attr("scrollable", Boolean.toString(node.isScrollable()));
        Rect bounds = new Rect();
        node.getBoundsInScreen(bounds);
        attr("bounds", "[" + bounds.left + "," + bounds.top + "][" + bounds.right + "," + bounds.bottom + "]");
        int children = node.getChildCount();
        for (int index = 0; index < children; index++) {
            if (count >= 3000 || ++requests > 3000 || output.getBuffer().length() >= 262144) {
                truncated = true;
                break;
            }
            AccessibilityNodeInfo child = node.getChild(index);
            if (child == null) { truncated = true; continue; }
            try { node(child, depth + 1); }
            finally { child.recycle(); }
        }
        if (node.getChildCount() != children) truncated = true;
        xml.endTag(null, "node");
    }

    /** A cut value is marked on its own node; the host keeps proofs about it conservative. */
    private void text(String name, CharSequence value, int limit) throws Exception {
        String text = value == null ? "" : value.toString();
        if (text.length() > limit) {
            // Avoid splitting a surrogate pair.
            int end = Character.isHighSurrogate(text.charAt(limit - 1)) ? limit - 1 : limit;
            xml.attribute(null, name, text.substring(0, end));
            xml.attribute(null, name + "-truncated", "true");
            return;
        }
        xml.attribute(null, name, text);
    }

    private void attr(String name, CharSequence value) throws Exception {
        String text = value == null ? "" : value.toString();
        if (text.length() > 1024) {
            // Avoid splitting a surrogate pair; omitted text invalidates negative proofs.
            int end = Character.isHighSurrogate(text.charAt(1023)) ? 1023 : 1024;
            text = text.substring(0, end);
            truncated = true;
        }
        xml.attribute(null, name, text);
    }
}

#include <gtk/gtk.h>
#include "faults.hpp"
namespace {
AcmeDesktopFaults faults;
GtkWidget *corrected, *armed, *status;
void run(GtkButton*, gpointer value) {
    faults.corrected = gtk_toggle_button_get_active(GTK_TOGGLE_BUTTON(corrected));
    faults.armed = gtk_toggle_button_get_active(GTK_TOGGLE_BUTTON(armed));
    std::string result;
    try { result = faults.run(GPOINTER_TO_INT(value)); } catch (const std::exception& error) { result = error.what(); }
    gtk_toggle_button_set_active(GTK_TOGGLE_BUTTON(armed), faults.armed);
    gtk_label_set_text(GTK_LABEL(status), result.c_str());
}
}
int main(int argc, char** argv) {
    gtk_init(&argc, &argv);
    GtkWidget* window = gtk_window_new(GTK_WINDOW_TOPLEVEL); gtk_window_set_title(GTK_WINDOW(window), "Acme Diagnostics"); gtk_window_set_default_size(GTK_WINDOW(window), 460, 480);
    GtkWidget* box = gtk_box_new(GTK_ORIENTATION_VERTICAL, 8); gtk_container_set_border_width(GTK_CONTAINER(box), 20); gtk_container_add(GTK_CONTAINER(window), box);
    gtk_box_pack_start(GTK_BOX(box), gtk_label_new("Acme native Linux diagnostics"), FALSE, FALSE, 0);
    corrected = gtk_check_button_new_with_label("Corrected mode"); armed = gtk_check_button_new_with_label("Arm app crash / 6-second hang");
    gtk_box_pack_start(GTK_BOX(box), corrected, FALSE, FALSE, 0); gtk_box_pack_start(GTK_BOX(box), armed, FALSE, FALSE, 0);
    const char* titles[] = {"Caught C++ exception", "Bounded CPU work", "Retain one MiB", "Reset", "C++ crash", "UI-thread hang (6 seconds)", "Background crash"};
    const int ids[] = {10,11,12,13,20,21,22};
    for (int n=0; n<7; ++n) { GtkWidget* button = gtk_button_new_with_label(titles[n]); gtk_box_pack_start(GTK_BOX(box), button, FALSE, FALSE, 0); g_signal_connect(button, "clicked", G_CALLBACK(run), GINT_TO_POINTER(ids[n])); }
    status = gtk_label_new("Acme ready"); gtk_box_pack_start(GTK_BOX(box), status, FALSE, FALSE, 0);
    g_signal_connect(window, "destroy", G_CALLBACK(gtk_main_quit), nullptr);
    gtk_widget_show_all(window); gtk_main(); faults.run(13); return 0;
}

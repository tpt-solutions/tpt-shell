// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Run-time binding to GTK 3, GDK, WebKit2GTK 4.1, JavaScriptCore, GLib, GObject, GIO and
//! Cairo (spec §6.3).
//!
//! Each function is looked up by name when [`load`] runs. Its C prototype is written out in
//! the table below, and the lookup is `unsafe` because the table is the only thing that says
//! the types are right. Nothing here links against those libraries, so the crate builds on a
//! machine without them, and a missing library or symbol becomes a `LoadError` that names it.
//!
//! GObjects and other opaque handles are `*mut c_void` throughout.

use std::ffi::{c_char, c_int, c_uint, c_ulong, c_void};
use std::sync::OnceLock;

use crate::loader::{LoadError, SharedLibrary};

/// An opaque GObject-derived pointer, or any other handle returned by these libraries.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub type Obj = *mut c_void;

/// `GSourceFunc` and `GClosureNotify`-style callbacks that take one pointer and return gboolean.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub type IdleFn = unsafe extern "C" fn(*mut c_void) -> c_int;

/// `GtkWindowType::GTK_WINDOW_TOPLEVEL`.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const GTK_WINDOW_TOPLEVEL: c_int = 0;
/// `GtkFileChooserAction`.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const GTK_FILE_CHOOSER_ACTION_OPEN: c_int = 0;
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const GTK_FILE_CHOOSER_ACTION_SAVE: c_int = 1;
/// `GtkResponseType` values used by the dialogs.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const GTK_RESPONSE_ACCEPT: c_int = -3;
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const GTK_RESPONSE_CANCEL: c_int = -6;
/// `WebKitPolicyDecisionType`.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const WEBKIT_POLICY_DECISION_NAVIGATION_ACTION: c_int = 0;
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const WEBKIT_POLICY_DECISION_NEW_WINDOW_ACTION: c_int = 1;
/// `WebKitUserContentInjectedFrames::WEBKIT_USER_CONTENT_INJECT_TOP_FRAME`.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const WEBKIT_INJECT_TOP_FRAME: c_int = 1;
/// `WebKitUserScriptInjectionTime::WEBKIT_USER_SCRIPT_INJECT_AT_DOCUMENT_START`.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const WEBKIT_INJECT_AT_DOCUMENT_START: c_int = 0;
/// `WebKitSnapshotRegion::WEBKIT_SNAPSHOT_REGION_VISIBLE` and `..._OPTIONS_NONE`.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const WEBKIT_SNAPSHOT_REGION_VISIBLE: c_int = 0;
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const WEBKIT_SNAPSHOT_OPTIONS_NONE: c_int = 0;
/// `cairo_status_t::CAIRO_STATUS_SUCCESS`.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const CAIRO_STATUS_SUCCESS: c_int = 0;

/// `GdkRGBA`, the colour WebKit uses for the page background.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Rgba {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

/// `GSList`, read to walk the file list a chooser returns.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
#[repr(C)]
pub struct GSList {
    pub data: *mut c_void,
    pub next: *mut GSList,
}

/// Libraries the GTK 3 adapter uses. Each is opened with the first name that loads.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const GTK3_LIB: &[&str] = &["libgtk-3.so.0"];
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const GDK3_LIB: &[&str] = &["libgdk-3.so.0"];
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const WEBKIT41_LIB: &[&str] = &["libwebkit2gtk-4.1.so.0"];
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const JSC41_LIB: &[&str] = &["libjavascriptcoregtk-4.1.so.0"];
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const GOBJECT_LIB: &[&str] = &["libgobject-2.0.so.0"];
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const GLIB_LIB: &[&str] = &["libglib-2.0.so.0"];
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const GIO_LIB: &[&str] = &["libgio-2.0.so.0"];
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const CAIRO_LIB: &[&str] = &["libcairo.so.2"];

/// The open libraries, one per name above.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub struct Libs {
    pub gtk: SharedLibrary,
    pub gdk: SharedLibrary,
    pub webkit: SharedLibrary,
    pub jsc: SharedLibrary,
    pub gobject: SharedLibrary,
    pub glib: SharedLibrary,
    pub gio: SharedLibrary,
    pub cairo: SharedLibrary,
}

macro_rules! bound_functions {
    ($( $lib:ident . $field:ident = $name:literal : fn($($arg:ty),*) -> $ret:ty; )*) => {
        /// Every function the adapter calls, looked up once.
        ///
        /// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux
        /// here; compile-checked.
        #[allow(non_snake_case, dead_code)]
        pub struct Fns {
            $( pub $field: unsafe extern "C" fn($($arg),*) -> $ret, )*
        }

        impl Fns {
            fn bind(libs: &Libs) -> Result<Fns, LoadError> {
                // SAFETY: each prototype above is the C declaration of the named function, as
                // documented by the library. The table is the contract.
                unsafe {
                    Ok(Fns {
                        $( $field: libs.$lib.symbol($name)?, )*
                    })
                }
            }
        }
    };
}

bound_functions! {
    // GTK 3
    gtk . gtk_init_check = "gtk_init_check": fn(*mut c_int, *mut c_void) -> c_int;
    gtk . gtk_main = "gtk_main": fn() -> ();
    gtk . gtk_main_quit = "gtk_main_quit": fn() -> ();
    gtk . gtk_main_iteration_do = "gtk_main_iteration_do": fn(c_int) -> c_int;
    gtk . gtk_events_pending = "gtk_events_pending": fn() -> c_int;
    gtk . gtk_window_new = "gtk_window_new": fn(c_int) -> Obj;
    gtk . gtk_window_set_title = "gtk_window_set_title": fn(Obj, *const c_char) -> ();
    gtk . gtk_window_set_default_size = "gtk_window_set_default_size": fn(Obj, c_int, c_int) -> ();
    gtk . gtk_window_resize = "gtk_window_resize": fn(Obj, c_int, c_int) -> ();
    gtk . gtk_window_get_size = "gtk_window_get_size": fn(Obj, *mut c_int, *mut c_int) -> ();
    gtk . gtk_window_move = "gtk_window_move": fn(Obj, c_int, c_int) -> ();
    gtk . gtk_window_get_position = "gtk_window_get_position": fn(Obj, *mut c_int, *mut c_int) -> ();
    gtk . gtk_window_fullscreen = "gtk_window_fullscreen": fn(Obj) -> ();
    gtk . gtk_window_unfullscreen = "gtk_window_unfullscreen": fn(Obj) -> ();
    gtk . gtk_window_set_decorated = "gtk_window_set_decorated": fn(Obj, c_int) -> ();
    gtk . gtk_window_set_keep_above = "gtk_window_set_keep_above": fn(Obj, c_int) -> ();
    gtk . gtk_window_set_transient_for = "gtk_window_set_transient_for": fn(Obj, Obj) -> ();
    gtk . gtk_window_set_modal = "gtk_window_set_modal": fn(Obj, c_int) -> ();
    gtk . gtk_widget_show_all = "gtk_widget_show_all": fn(Obj) -> ();
    gtk . gtk_widget_show = "gtk_widget_show": fn(Obj) -> ();
    gtk . gtk_widget_hide = "gtk_widget_hide": fn(Obj) -> ();
    gtk . gtk_widget_destroy = "gtk_widget_destroy": fn(Obj) -> ();
    gtk . gtk_widget_set_app_paintable = "gtk_widget_set_app_paintable": fn(Obj, c_int) -> ();
    gtk . gtk_widget_set_visual = "gtk_widget_set_visual": fn(Obj, Obj) -> ();
    gtk . gtk_widget_get_screen = "gtk_widget_get_screen": fn(Obj) -> Obj;
    gtk . gtk_container_add = "gtk_container_add": fn(Obj, Obj) -> ();
    gtk . gtk_clipboard_get = "gtk_clipboard_get": fn(Obj) -> Obj;
    gtk . gtk_clipboard_set_text = "gtk_clipboard_set_text": fn(Obj, *const c_char, c_int) -> ();
    gtk . gtk_clipboard_wait_for_text = "gtk_clipboard_wait_for_text": fn(Obj) -> *mut c_char;
    gtk . gtk_clipboard_store = "gtk_clipboard_store": fn(Obj) -> ();
    gtk . gtk_dialog_new = "gtk_dialog_new": fn() -> Obj;
    gtk . gtk_dialog_add_button = "gtk_dialog_add_button": fn(Obj, *const c_char, c_int) -> Obj;
    gtk . gtk_dialog_get_content_area = "gtk_dialog_get_content_area": fn(Obj) -> Obj;
    gtk . gtk_dialog_run = "gtk_dialog_run": fn(Obj) -> c_int;
    gtk . gtk_file_chooser_widget_new = "gtk_file_chooser_widget_new": fn(c_int) -> Obj;
    gtk . gtk_file_chooser_set_select_multiple = "gtk_file_chooser_set_select_multiple": fn(Obj, c_int) -> ();
    gtk . gtk_file_chooser_set_current_name = "gtk_file_chooser_set_current_name": fn(Obj, *const c_char) -> ();
    gtk . gtk_file_chooser_set_do_overwrite_confirmation = "gtk_file_chooser_set_do_overwrite_confirmation": fn(Obj, c_int) -> ();
    gtk . gtk_file_chooser_add_filter = "gtk_file_chooser_add_filter": fn(Obj, Obj) -> ();
    gtk . gtk_file_chooser_get_filename = "gtk_file_chooser_get_filename": fn(Obj) -> *mut c_char;
    gtk . gtk_file_chooser_get_filenames = "gtk_file_chooser_get_filenames": fn(Obj) -> *mut GSList;
    gtk . gtk_file_filter_new = "gtk_file_filter_new": fn() -> Obj;
    gtk . gtk_file_filter_set_name = "gtk_file_filter_set_name": fn(Obj, *const c_char) -> ();
    gtk . gtk_file_filter_add_pattern = "gtk_file_filter_add_pattern": fn(Obj, *const c_char) -> ();
    // GDK
    gdk . gdk_screen_get_rgba_visual = "gdk_screen_get_rgba_visual": fn(Obj) -> Obj;
    gdk . gdk_screen_is_composited = "gdk_screen_is_composited": fn(Obj) -> c_int;
    gdk . gdk_atom_intern = "gdk_atom_intern": fn(*const c_char, c_int) -> Obj;
    // WebKit2GTK 4.1
    webkit . webkit_web_view_new = "webkit_web_view_new": fn() -> Obj;
    webkit . webkit_web_view_new_with_context = "webkit_web_view_new_with_context": fn(Obj) -> Obj;
    webkit . webkit_web_context_new_ephemeral = "webkit_web_context_new_ephemeral": fn() -> Obj;
    webkit . webkit_web_view_load_html = "webkit_web_view_load_html": fn(Obj, *const c_char, *const c_char) -> ();
    webkit . webkit_web_view_load_uri = "webkit_web_view_load_uri": fn(Obj, *const c_char) -> ();
    webkit . webkit_web_view_reload = "webkit_web_view_reload": fn(Obj) -> ();
    webkit . webkit_web_view_get_user_content_manager = "webkit_web_view_get_user_content_manager": fn(Obj) -> Obj;
    webkit . webkit_web_view_get_context = "webkit_web_view_get_context": fn(Obj) -> Obj;
    webkit . webkit_web_view_run_javascript = "webkit_web_view_run_javascript": fn(Obj, *const c_char, Obj, *const c_void, *mut c_void) -> ();
    webkit . webkit_web_view_set_background_color = "webkit_web_view_set_background_color": fn(Obj, *const Rgba) -> ();
    webkit . webkit_web_view_get_snapshot = "webkit_web_view_get_snapshot": fn(Obj, c_int, c_int, Obj, *const c_void, *mut c_void) -> ();
    webkit . webkit_web_view_get_snapshot_finish = "webkit_web_view_get_snapshot_finish": fn(Obj, Obj, *mut Obj) -> Obj;
    webkit . webkit_user_content_manager_register_script_message_handler = "webkit_user_content_manager_register_script_message_handler": fn(Obj, *const c_char) -> c_int;
    webkit . webkit_user_content_manager_add_script = "webkit_user_content_manager_add_script": fn(Obj, Obj) -> ();
    webkit . webkit_user_script_new = "webkit_user_script_new": fn(*const c_char, c_int, c_int, *const *const c_char, *const *const c_char) -> Obj;
    webkit . webkit_user_script_unref = "webkit_user_script_unref": fn(Obj) -> ();
    webkit . webkit_web_context_register_uri_scheme = "webkit_web_context_register_uri_scheme": fn(Obj, *const c_char, *const c_void, *mut c_void, *const c_void) -> ();
    webkit . webkit_uri_scheme_request_get_uri = "webkit_uri_scheme_request_get_uri": fn(Obj) -> *const c_char;
    webkit . webkit_uri_scheme_request_get_web_view = "webkit_uri_scheme_request_get_web_view": fn(Obj) -> Obj;
    webkit . webkit_uri_scheme_request_finish = "webkit_uri_scheme_request_finish": fn(Obj, Obj, i64, *const c_char) -> ();
    webkit . webkit_uri_scheme_request_finish_error = "webkit_uri_scheme_request_finish_error": fn(Obj, Obj) -> ();
    webkit . webkit_policy_decision_use = "webkit_policy_decision_use": fn(Obj) -> ();
    webkit . webkit_policy_decision_ignore = "webkit_policy_decision_ignore": fn(Obj) -> ();
    webkit . webkit_navigation_policy_decision_get_navigation_action = "webkit_navigation_policy_decision_get_navigation_action": fn(Obj) -> Obj;
    webkit . webkit_navigation_action_get_request = "webkit_navigation_action_get_request": fn(Obj) -> Obj;
    webkit . webkit_uri_request_get_uri = "webkit_uri_request_get_uri": fn(Obj) -> *const c_char;
    webkit . webkit_javascript_result_get_js_value = "webkit_javascript_result_get_js_value": fn(Obj) -> Obj;
    // JavaScriptCore
    jsc . jsc_value_to_string = "jsc_value_to_string": fn(Obj) -> *mut c_char;
    // GObject
    gobject . g_signal_connect_data = "g_signal_connect_data": fn(Obj, *const c_char, *const c_void, *mut c_void, *const c_void, c_int) -> c_ulong;
    gobject . g_object_unref = "g_object_unref": fn(Obj) -> ();
    gobject . g_object_set_data = "g_object_set_data": fn(Obj, *const c_char, *mut c_void) -> ();
    gobject . g_object_get_data = "g_object_get_data": fn(Obj, *const c_char) -> *mut c_void;
    // GLib
    glib . g_idle_add = "g_idle_add": fn(IdleFn, *mut c_void) -> c_uint;
    glib . g_free = "g_free": fn(*mut c_void) -> ();
    glib . g_malloc = "g_malloc": fn(usize) -> *mut c_void;
    glib . g_quark_from_string = "g_quark_from_string": fn(*const c_char) -> c_uint;
    glib . g_error_new_literal = "g_error_new_literal": fn(c_uint, c_int, *const c_char) -> Obj;
    glib . g_error_free = "g_error_free": fn(Obj) -> ();
    glib . g_slist_free = "g_slist_free": fn(*mut GSList) -> ();
    // GIO
    gio . g_memory_input_stream_new_from_data = "g_memory_input_stream_new_from_data": fn(*const c_void, usize, *const c_void) -> Obj;
    // Cairo
    cairo . cairo_surface_write_to_png_stream = "cairo_surface_write_to_png_stream": fn(Obj, *const c_void, *mut c_void) -> c_int;
    cairo . cairo_surface_destroy = "cairo_surface_destroy": fn(Obj) -> ();
}

/// The bound GTK 3 and WebKit 4.1 adapter. Loaded once per process and never unloaded, since
/// unloading GTK while its callbacks may still run is unsafe. GTK itself is not thread-safe:
/// only the main loop proxy, which calls the thread-safe `g_idle_add`, is used off the UI thread.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub struct Gtk3 {
    pub libs: Libs,
    pub fns: Fns,
}

static GTK3: OnceLock<Result<Gtk3, LoadError>> = OnceLock::new();

fn open_all() -> Result<Gtk3, LoadError> {
    let libs = Libs {
        gtk: SharedLibrary::open(GTK3_LIB)?,
        gdk: SharedLibrary::open(GDK3_LIB)?,
        webkit: SharedLibrary::open(WEBKIT41_LIB)?,
        jsc: SharedLibrary::open(JSC41_LIB)?,
        gobject: SharedLibrary::open(GOBJECT_LIB)?,
        glib: SharedLibrary::open(GLIB_LIB)?,
        gio: SharedLibrary::open(GIO_LIB)?,
        cairo: SharedLibrary::open(CAIRO_LIB)?,
    };
    let fns = Fns::bind(&libs)?;
    Ok(Gtk3 { libs, fns })
}

/// The GTK 3 and WebKit 4.1 adapter, or the reason it could not be loaded. The first call
/// tries the libraries; later calls return the same result.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub fn load() -> Result<&'static Gtk3, &'static LoadError> {
    GTK3.get_or_init(open_all).as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prototypes_have_the_sizes_the_c_side_expects() {
        assert_eq!(std::mem::size_of::<Rgba>(), 32);
        assert_eq!(std::mem::size_of::<GSList>(), 16);
    }
}

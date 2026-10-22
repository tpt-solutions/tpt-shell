// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Run-time binding to GTK 4 (which holds GDK), WebKitGTK 6.0, JavaScriptCore 6.0, GLib, GObject
//! and GIO (spec §6.3).
//!
//! Same method as `gtk3::sys`: each function is looked up by name when [`load`] runs, its C
//! prototype is written in the tables below, and nothing links against the libraries. GTK 4.10
//! added `GtkFileDialog`; those functions sit in a second table ([`Gtk4::dialog_fns`]) so an older
//! GTK 4 (Ubuntu 22.04 has 4.6) still loads and only the file dialogs report `Unsupported`.
//!
//! GObjects and other opaque handles are `*mut c_void` throughout.

use std::ffi::{c_char, c_int, c_uint, c_ulong, c_void};
use std::sync::OnceLock;

use crate::loader::{LoadError, SharedLibrary};

/// An opaque GObject-derived pointer, or any other handle returned by these libraries.
///
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub type Obj = *mut c_void;

/// `GSourceFunc`-style callback that takes one pointer and returns gboolean.
///
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub type IdleFn = unsafe extern "C" fn(*mut c_void) -> c_int;

/// `WebKitPolicyDecisionType`.
///
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const WEBKIT_POLICY_DECISION_NAVIGATION_ACTION: c_int = 0;
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const WEBKIT_POLICY_DECISION_NEW_WINDOW_ACTION: c_int = 1;
/// `WebKitUserContentInjectedFrames::WEBKIT_USER_CONTENT_INJECT_TOP_FRAME`.
///
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const WEBKIT_INJECT_TOP_FRAME: c_int = 1;
/// `WebKitUserScriptInjectionTime::WEBKIT_USER_SCRIPT_INJECT_AT_DOCUMENT_START`.
///
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const WEBKIT_INJECT_AT_DOCUMENT_START: c_int = 0;
/// `WebKitSnapshotRegion::WEBKIT_SNAPSHOT_REGION_VISIBLE` and `..._OPTIONS_NONE`.
///
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const WEBKIT_SNAPSHOT_REGION_VISIBLE: c_int = 0;
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const WEBKIT_SNAPSHOT_OPTIONS_NONE: c_int = 0;
/// `WebKitWebProcessTerminationReason::WEBKIT_WEB_PROCESS_TERMINATED_BY_API`: the app asked for
/// the termination, so the page is not reloaded.
///
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const WEBKIT_WEB_PROCESS_TERMINATED_BY_API: c_int = 2;
/// `GTK_STYLE_PROVIDER_PRIORITY_APPLICATION`.
///
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const GTK_STYLE_PROVIDER_PRIORITY_APPLICATION: c_uint = 600;
/// `G_PRIORITY_DEFAULT`.
///
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const G_PRIORITY_DEFAULT: c_int = 0;

/// `GdkRGBA`, the colour WebKit uses for the page background.
///
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Rgba {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub alpha: f32,
}

/// Libraries the GTK 4 adapter uses. Each is opened with the first name that loads.
///
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const GTK4_LIB: &[&str] = &["libgtk-4.so.1"];
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const WEBKIT6_LIB: &[&str] = &["libwebkitgtk-6.0.so.4"];
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const JSC6_LIB: &[&str] = &["libjavascriptcoregtk-6.0.so.1"];
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const GOBJECT_LIB: &[&str] = &["libgobject-2.0.so.0"];
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const GLIB_LIB: &[&str] = &["libglib-2.0.so.0"];
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub const GIO_LIB: &[&str] = &["libgio-2.0.so.0"];

/// The open libraries, one per name above.
///
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub struct Libs {
    pub gtk: SharedLibrary,
    pub webkit: SharedLibrary,
    pub jsc: SharedLibrary,
    pub gobject: SharedLibrary,
    pub glib: SharedLibrary,
    pub gio: SharedLibrary,
}

macro_rules! bound_functions {
    ($name:ident, $doc:literal; $( $lib:ident . $field:ident = $sym:literal : fn($($arg:ty),*) -> $ret:ty; )*) => {
        #[doc = $doc]
        ///
        /// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux
        /// here; compile-checked.
        #[allow(non_snake_case, dead_code)]
        pub struct $name {
            $( pub $field: unsafe extern "C" fn($($arg),*) -> $ret, )*
        }

        impl $name {
            fn bind(libs: &Libs) -> Result<$name, LoadError> {
                // SAFETY: each prototype above is the C declaration of the named function, as
                // documented by the library. The table is the contract.
                unsafe {
                    Ok($name {
                        $( $field: libs.$lib.symbol($sym)?, )*
                    })
                }
            }
        }
    };
}

bound_functions! { Fns, "Every function the adapter always needs, looked up once.";
    // GTK 4
    gtk . gtk_init_check = "gtk_init_check": fn() -> c_int;
    gtk . gtk_window_new = "gtk_window_new": fn() -> Obj;
    gtk . gtk_window_set_title = "gtk_window_set_title": fn(Obj, *const c_char) -> ();
    gtk . gtk_window_set_default_size = "gtk_window_set_default_size": fn(Obj, c_int, c_int) -> ();
    gtk . gtk_window_set_child = "gtk_window_set_child": fn(Obj, Obj) -> ();
    gtk . gtk_window_present = "gtk_window_present": fn(Obj) -> ();
    gtk . gtk_window_destroy = "gtk_window_destroy": fn(Obj) -> ();
    gtk . gtk_window_fullscreen = "gtk_window_fullscreen": fn(Obj) -> ();
    gtk . gtk_window_unfullscreen = "gtk_window_unfullscreen": fn(Obj) -> ();
    gtk . gtk_window_set_decorated = "gtk_window_set_decorated": fn(Obj, c_int) -> ();
    gtk . gtk_widget_set_visible = "gtk_widget_set_visible": fn(Obj, c_int) -> ();
    gtk . gtk_widget_get_width = "gtk_widget_get_width": fn(Obj) -> c_int;
    gtk . gtk_widget_get_height = "gtk_widget_get_height": fn(Obj) -> c_int;
    gtk . gtk_widget_add_css_class = "gtk_widget_add_css_class": fn(Obj, *const c_char) -> ();
    gtk . gtk_widget_remove_css_class = "gtk_widget_remove_css_class": fn(Obj, *const c_char) -> ();
    gtk . gtk_css_provider_new = "gtk_css_provider_new": fn() -> Obj;
    gtk . gtk_css_provider_load_from_data = "gtk_css_provider_load_from_data": fn(Obj, *const c_char, isize) -> ();
    gtk . gtk_style_context_add_provider_for_display = "gtk_style_context_add_provider_for_display": fn(Obj, Obj, c_uint) -> ();
    // GDK 4 (inside libgtk-4)
    gtk . gdk_display_get_default = "gdk_display_get_default": fn() -> Obj;
    gtk . gdk_display_get_clipboard = "gdk_display_get_clipboard": fn(Obj) -> Obj;
    gtk . gdk_display_is_composited = "gdk_display_is_composited": fn(Obj) -> c_int;
    gtk . gdk_clipboard_set_text = "gdk_clipboard_set_text": fn(Obj, *const c_char) -> ();
    gtk . gdk_clipboard_read_text_async = "gdk_clipboard_read_text_async": fn(Obj, Obj, *const c_void, *mut c_void) -> ();
    gtk . gdk_clipboard_read_text_finish = "gdk_clipboard_read_text_finish": fn(Obj, Obj, *mut Obj) -> *mut c_char;
    gtk . gdk_clipboard_store_async = "gdk_clipboard_store_async": fn(Obj, c_int, Obj, *const c_void, *mut c_void) -> ();
    gtk . gdk_texture_save_to_png_bytes = "gdk_texture_save_to_png_bytes": fn(Obj) -> Obj;
    // WebKitGTK 6.0
    webkit . webkit_web_view_new = "webkit_web_view_new": fn() -> Obj;
    webkit . webkit_web_view_get_type = "webkit_web_view_get_type": fn() -> usize;
    webkit . webkit_web_context_new = "webkit_web_context_new": fn() -> Obj;
    webkit . webkit_network_session_new_ephemeral = "webkit_network_session_new_ephemeral": fn() -> Obj;
    webkit . webkit_web_view_load_html = "webkit_web_view_load_html": fn(Obj, *const c_char, *const c_char) -> ();
    webkit . webkit_web_view_load_uri = "webkit_web_view_load_uri": fn(Obj, *const c_char) -> ();
    webkit . webkit_web_view_reload = "webkit_web_view_reload": fn(Obj) -> ();
    webkit . webkit_web_view_get_user_content_manager = "webkit_web_view_get_user_content_manager": fn(Obj) -> Obj;
    webkit . webkit_web_view_get_context = "webkit_web_view_get_context": fn(Obj) -> Obj;
    webkit . webkit_web_view_evaluate_javascript = "webkit_web_view_evaluate_javascript": fn(Obj, *const c_char, isize, *const c_char, *const c_char, Obj, *const c_void, *mut c_void) -> ();
    webkit . webkit_web_view_set_background_color = "webkit_web_view_set_background_color": fn(Obj, *const Rgba) -> ();
    webkit . webkit_web_view_get_snapshot = "webkit_web_view_get_snapshot": fn(Obj, c_int, c_int, Obj, *const c_void, *mut c_void) -> ();
    webkit . webkit_web_view_get_snapshot_finish = "webkit_web_view_get_snapshot_finish": fn(Obj, Obj, *mut Obj) -> Obj;
    webkit . webkit_user_content_manager_register_script_message_handler = "webkit_user_content_manager_register_script_message_handler": fn(Obj, *const c_char, *const c_char) -> c_int;
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
    // JavaScriptCore 6.0
    jsc . jsc_value_to_string = "jsc_value_to_string": fn(Obj) -> *mut c_char;
    // GObject
    gobject . g_signal_connect_data = "g_signal_connect_data": fn(Obj, *const c_char, *const c_void, *mut c_void, *const c_void, c_int) -> c_ulong;
    gobject . g_object_unref = "g_object_unref": fn(Obj) -> ();
    gobject . g_object_set_data = "g_object_set_data": fn(Obj, *const c_char, *mut c_void) -> ();
    gobject . g_object_get_data = "g_object_get_data": fn(Obj, *const c_char) -> *mut c_void;
    // GLib
    glib . g_main_loop_new = "g_main_loop_new": fn(Obj, c_int) -> Obj;
    glib . g_main_loop_run = "g_main_loop_run": fn(Obj) -> ();
    glib . g_main_loop_quit = "g_main_loop_quit": fn(Obj) -> ();
    glib . g_main_loop_unref = "g_main_loop_unref": fn(Obj) -> ();
    glib . g_main_context_iteration = "g_main_context_iteration": fn(Obj, c_int) -> c_int;
    glib . g_idle_add = "g_idle_add": fn(IdleFn, *mut c_void) -> c_uint;
    glib . g_free = "g_free": fn(*mut c_void) -> ();
    glib . g_malloc = "g_malloc": fn(usize) -> *mut c_void;
    glib . g_quark_from_string = "g_quark_from_string": fn(*const c_char) -> c_uint;
    glib . g_error_new_literal = "g_error_new_literal": fn(c_uint, c_int, *const c_char) -> Obj;
    glib . g_error_free = "g_error_free": fn(Obj) -> ();
    glib . g_bytes_get_data = "g_bytes_get_data": fn(Obj, *mut usize) -> *const c_void;
    glib . g_bytes_unref = "g_bytes_unref": fn(Obj) -> ();
    // GIO
    gio . g_memory_input_stream_new_from_data = "g_memory_input_stream_new_from_data": fn(*const c_void, usize, *const c_void) -> Obj;
}

bound_functions! { DialogFns, "The `GtkFileDialog` functions (GTK 4.10 and later) and the `GListModel` and `GFile` calls that read their results.";
    gtk . gtk_file_dialog_new = "gtk_file_dialog_new": fn() -> Obj;
    gtk . gtk_file_dialog_set_title = "gtk_file_dialog_set_title": fn(Obj, *const c_char) -> ();
    gtk . gtk_file_dialog_set_modal = "gtk_file_dialog_set_modal": fn(Obj, c_int) -> ();
    gtk . gtk_file_dialog_set_filters = "gtk_file_dialog_set_filters": fn(Obj, Obj) -> ();
    gtk . gtk_file_dialog_set_initial_name = "gtk_file_dialog_set_initial_name": fn(Obj, *const c_char) -> ();
    gtk . gtk_file_dialog_open = "gtk_file_dialog_open": fn(Obj, Obj, Obj, *const c_void, *mut c_void) -> ();
    gtk . gtk_file_dialog_open_finish = "gtk_file_dialog_open_finish": fn(Obj, Obj, *mut Obj) -> Obj;
    gtk . gtk_file_dialog_open_multiple = "gtk_file_dialog_open_multiple": fn(Obj, Obj, Obj, *const c_void, *mut c_void) -> ();
    gtk . gtk_file_dialog_open_multiple_finish = "gtk_file_dialog_open_multiple_finish": fn(Obj, Obj, *mut Obj) -> Obj;
    gtk . gtk_file_dialog_save = "gtk_file_dialog_save": fn(Obj, Obj, Obj, *const c_void, *mut c_void) -> ();
    gtk . gtk_file_dialog_save_finish = "gtk_file_dialog_save_finish": fn(Obj, Obj, *mut Obj) -> Obj;
    gtk . gtk_file_filter_new = "gtk_file_filter_new": fn() -> Obj;
    gtk . gtk_file_filter_set_name = "gtk_file_filter_set_name": fn(Obj, *const c_char) -> ();
    gtk . gtk_file_filter_add_pattern = "gtk_file_filter_add_pattern": fn(Obj, *const c_char) -> ();
    gtk . gtk_file_filter_get_type = "gtk_file_filter_get_type": fn() -> usize;
    gio . g_list_store_new = "g_list_store_new": fn(usize) -> Obj;
    gio . g_list_store_append = "g_list_store_append": fn(Obj, Obj) -> ();
    gio . g_list_model_get_n_items = "g_list_model_get_n_items": fn(Obj) -> c_uint;
    gio . g_list_model_get_item = "g_list_model_get_item": fn(Obj, c_uint) -> Obj;
    gio . g_file_get_path = "g_file_get_path": fn(Obj) -> *mut c_char;
}

/// The bound GTK 4 and WebKitGTK 6.0 adapter. Loaded once per process and never unloaded, since
/// unloading GTK while its callbacks may still run is unsafe. GTK itself is not thread-safe:
/// only the main loop proxy, which calls the thread-safe `g_idle_add`, is used off the UI thread.
///
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub struct Gtk4 {
    pub libs: Libs,
    pub fns: Fns,
    /// `g_object_new`, which is variadic, so it is bound by hand: the table macro only makes
    /// fixed-argument prototypes.
    pub g_object_new: unsafe extern "C" fn(usize, *const c_char, ...) -> Obj,
    dialogs: Result<DialogFns, LoadError>,
}

impl Gtk4 {
    /// The `GtkFileDialog` functions, or why this GTK 4 does not have them (it is older than
    /// 4.10).
    ///
    /// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
    /// compile-checked.
    pub fn dialog_fns(&self) -> Result<&DialogFns, &LoadError> {
        self.dialogs.as_ref()
    }
}

static GTK4: OnceLock<Result<Gtk4, LoadError>> = OnceLock::new();

fn open_all() -> Result<Gtk4, LoadError> {
    let libs = Libs {
        gtk: SharedLibrary::open(GTK4_LIB)?,
        webkit: SharedLibrary::open(WEBKIT6_LIB)?,
        jsc: SharedLibrary::open(JSC6_LIB)?,
        gobject: SharedLibrary::open(GOBJECT_LIB)?,
        glib: SharedLibrary::open(GLIB_LIB)?,
        gio: SharedLibrary::open(GIO_LIB)?,
    };
    let fns = Fns::bind(&libs)?;
    let dialogs = DialogFns::bind(&libs);
    // SAFETY: `g_object_new` is declared in GObject as `gpointer g_object_new(GType type,
    // const gchar *first_property_name, ...)`, which is this prototype.
    let g_object_new = unsafe {
        libs.gobject
            .symbol::<unsafe extern "C" fn(usize, *const c_char, ...) -> Obj>("g_object_new")?
    };
    Ok(Gtk4 {
        libs,
        fns,
        g_object_new,
        dialogs,
    })
}

/// The GTK 4 and WebKitGTK 6.0 adapter, or the reason it could not be loaded. The first call
/// tries the libraries; later calls return the same result.
///
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub fn load() -> Result<&'static Gtk4, &'static LoadError> {
    GTK4.get_or_init(open_all).as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prototypes_have_the_sizes_the_c_side_expects() {
        // GTK 4's GdkRGBA is four floats, unlike GTK 3's four doubles.
        assert_eq!(std::mem::size_of::<Rgba>(), 16);
    }
}

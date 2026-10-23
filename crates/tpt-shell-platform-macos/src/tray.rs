// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! A menu bar icon (`NSStatusItem`) with a menu of text items. Choosing an item calls back
//! with its index. Declared with `objc2::define_class!`, used on the main thread only.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{
    define_class, msg_send, sel, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
use objc2_foundation::{NSData, NSObject, NSObjectProtocol, NSString};

/// What the tray reports when the user picks a menu item: its index in the list given to
/// [`Tray::new`].
///
/// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
pub type TraySelect = Box<dyn Fn(usize)>;

/// `NSVariableStatusItemLength`: the item is as wide as its content.
const VARIABLE_LENGTH: f64 = -1.0;

/// Menu item title that is drawn as a separator line.
///
/// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
pub const SEPARATOR: &str = "-";

/// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
pub struct TrayTargetIvars {
    on_select: TraySelect,
}

define_class!(
    /// Receives the menu item action and forwards the item's tag.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TptTrayTarget"]
    #[ivars = TrayTargetIvars]
    pub struct TrayTarget;

    unsafe impl NSObjectProtocol for TrayTarget {}

    impl TrayTarget {
        #[unsafe(method(itemChosen:))]
        fn item_chosen(&self, sender: &NSMenuItem) {
            let tag = sender.tag();
            if let Ok(index) = usize::try_from(tag) {
                (self.ivars().on_select)(index);
            }
        }
    }
);

impl TrayTarget {
    fn new(mtm: MainThreadMarker, on_select: TraySelect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TrayTargetIvars { on_select });
        // SAFETY: NSObject's init, on a freshly allocated instance of this class.
        unsafe { msg_send![super(this), init] }
    }
}

/// A status item in the menu bar. Dropping it removes the icon.
///
/// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
pub struct Tray {
    item: Retained<NSStatusItem>,
    // The menu items hold their target weakly, so this keeps it alive.
    _target: Retained<TrayTarget>,
    _menu: Retained<NSMenu>,
}

impl Tray {
    /// Adds an icon to the menu bar. `title` is shown when there is no `icon_png`. `items` are
    /// the menu entries; [`SEPARATOR`] draws a line. `on_select` gets the index of the chosen
    /// entry. Returns an error when the icon bytes are not an image.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn new(
        mtm: MainThreadMarker,
        title: &str,
        tooltip: &str,
        icon_png: Option<&[u8]>,
        items: &[&str],
        on_select: TraySelect,
    ) -> Result<Self, String> {
        let target = TrayTarget::new(mtm, on_select);
        let menu = NSMenu::new(mtm);
        for (index, text) in items.iter().enumerate() {
            if *text == SEPARATOR {
                menu.addItem(&NSMenuItem::separatorItem(mtm));
                continue;
            }
            // SAFETY: the selector names the method declared on TrayTarget above, and the
            // target outlives the menu through the struct field.
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(text),
                    Some(sel!(itemChosen:)),
                    &NSString::from_str(""),
                )
            };
            // SAFETY: as above.
            unsafe { item.setTarget(Some(&*target as &AnyObject)) };
            item.setTag(index as isize);
            menu.addItem(&item);
        }

        let item = NSStatusBar::systemStatusBar().statusItemWithLength(VARIABLE_LENGTH);
        item.setMenu(Some(&menu));
        let button = item
            .button(mtm)
            .ok_or_else(|| "the status item has no button".to_string())?;
        match icon_png {
            Some(bytes) => {
                let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes))
                    .ok_or_else(|| "the tray icon is not an image".to_string())?;
                // A template image follows the menu bar's light or dark style.
                image.setTemplate(true);
                button.setImage(Some(&image));
            }
            None => button.setTitle(&NSString::from_str(title)),
        }
        button.setToolTip(Some(&NSString::from_str(tooltip)));
        Ok(Tray {
            item,
            _target: target,
            _menu: menu,
        })
    }

    /// Shows or hides the icon without removing it.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn set_visible(&self, visible: bool) {
        self.item.setVisible(visible);
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}

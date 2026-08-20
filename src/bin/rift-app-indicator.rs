//! A small, standalone menu-bar application indicator for Rift.
//!
//! Rift owns the window state; this process only queries it over Mach IPC and
//! refreshes when Rift publishes a relevant event.

use std::cell::RefCell;
use std::collections::HashSet;
use std::thread;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{
    define_class, msg_send, sel, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSFont, NSFontAttributeName, NSGraphicsContext,
    NSImage, NSMenu, NSMenuItem, NSRunningApplication, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength, NSView, NSWorkspace,
};
use objc2_core_foundation::{CFAttributedString, CFDictionary, CFString, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGContext;
use objc2_core_text::CTLine;
use objc2_foundation::{
    NSAttributedStringKey, NSDictionary, NSMutableDictionary, NSObject, NSString,
};
use rift_client::{EventKind, RiftMachClient};

struct IndicatorIvars {
    status_item: Retained<NSStatusItem>,
    view: Retained<IndicatorView>,
    menu: RefCell<Option<Retained<NSMenu>>>,
}

struct IndicatorViewIvars {
    groups: RefCell<Vec<(u64, Vec<Retained<NSImage>>)>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "RiftAppIndicatorView"]
    #[ivars = IndicatorViewIvars]
    struct IndicatorView;

    impl IndicatorView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: objc2_foundation::NSRect) {
            let Some(context) = NSGraphicsContext::currentContext() else {
                return;
            };
            let cg = context.CGContext();
            let cg = cg.as_ref();
            CGContext::clear_rect(Some(cg), self.bounds());
            let mut x = 0.0;
            let groups = self.ivars().groups.borrow();
            for (workspace_number, group) in groups.iter() {
                draw_workspace_number(cg, *workspace_number, x);
                x += 12.0;
                for icon in group {
                    let rect = CGRect::new(CGPoint::new(x, 0.0), CGSize::new(17.0, 17.0));
                    let _: () = unsafe { msg_send![&**icon, drawInRect: rect] };
                    x += 19.0;
                }
            }
        }

    }
);

impl IndicatorView {
    fn set_groups(&self, groups: Vec<(u64, Vec<Retained<NSImage>>)>) {
        let icon_count = groups.iter().map(|(_, icons)| icons.len()).sum::<usize>();
        let width = groups.len() as f64 * 12.0 + icon_count as f64 * 19.0;
        *self.ivars().groups.borrow_mut() = groups;
        self.setFrameSize(objc2_foundation::NSSize::new(width, 17.0));
        self.setNeedsDisplay(true);
    }
}

fn as_any_object<T: objc2::Message>(object: &T) -> &AnyObject {
    unsafe { &*(object as *const T as *const AnyObject) }
}

fn draw_workspace_number(context: &CGContext, number: u64, x: f64) {
    let font = NSFont::menuBarFontOfSize(11.0);
    let color = objc2_app_kit::NSColor::labelColor();
    let attrs = NSMutableDictionary::<NSAttributedStringKey, AnyObject>::new();
    unsafe {
        attrs.setObject_forKeyedSubscript(
            Some(as_any_object(&*font)),
            ProtocolObject::from_ref(NSFontAttributeName),
        );
        attrs.setObject_forKeyedSubscript(
            Some(as_any_object(&*color)),
            ProtocolObject::from_ref(objc2_app_kit::NSForegroundColorAttributeName),
        );
    }
    let text = NSString::from_str(&number.to_string());
    let cf_string: &CFString = text.as_ref();
    let attrs: Retained<NSDictionary<NSAttributedStringKey, AnyObject>> =
        unsafe { Retained::cast_unchecked(attrs) };
    let cf_dict_ref: &CFDictionary<NSAttributedStringKey, AnyObject> = attrs.as_ref();
    let cf_dict = cf_dict_ref.as_opaque();
    let Some(line) = (unsafe { CFAttributedString::new(None, Some(cf_string), Some(cf_dict)) })
        .map(|string| unsafe { CTLine::with_attributed_string(string.as_ref()) })
    else {
        return;
    };
    CGContext::set_text_position(Some(context), x as _, 2.0);
    let line_ref: &CTLine = line.as_ref();
    unsafe { line_ref.draw(context) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RiftAppIndicator"]
    #[ivars = IndicatorIvars]
    struct Indicator;

    impl Indicator {
        #[unsafe(method(refresh:))]
        fn refresh(&self, _sender: Option<&AnyObject>) {
            let Ok(client) = RiftMachClient::connect() else {
                return;
            };
            let Ok(workspaces) = client.get_workspaces(None) else {
                return;
            };

            let mtm = MainThreadMarker::new().expect("indicator must run on the main thread");
            let title = NSString::from_str("Rift Applications by Workspace");
            let menu: Retained<NSMenu> = unsafe {
                msg_send![NSMenu::alloc(mtm), initWithTitle: &*title]
            };
            let mut status_groups = Vec::new();

            for (workspace_index, workspace) in workspaces.iter().enumerate() {
                if workspace_index > 0 {
                    menu.addItem(&separator());
                }

                let index = workspace
                    .index
                    .checked_add(1)
                    .map(|index| index as u64)
                    .unwrap_or(workspace_index as u64 + 1);
                let name = &workspace.name;
                let active = workspace.is_active;
                let heading = if name.is_empty() {
                    format!("{}Workspace {}", if active { "● " } else { "" }, index)
                } else {
                    format!("{}Workspace {}: {}", if active { "● " } else { "" }, index, name)
                };
                let heading = menu_item(mtm, &heading, None);
                heading.setEnabled(false);
                menu.addItem(&heading);

                let mut seen = HashSet::new();
                let mut applications = Vec::new();
                let mut group_icons = Vec::new();
                for window in &workspace.windows {
                        let bundle_id = window.bundle_id.as_deref();
                        let app_name = window.app_name.as_deref();
                        let Some(key) = bundle_id.or(app_name) else {
                            continue;
                        };
                        if !seen.insert(key.to_owned()) {
                            continue;
                        }
                        applications.push((
                            bundle_id.map(str::to_owned),
                            app_name.unwrap_or(key).to_owned(),
                        ));
                }

                applications.sort_by(|left, right| {
                    left.1.to_lowercase().cmp(&right.1.to_lowercase())
                });
                if applications.is_empty() {
                    let item = menu_item(mtm, "No applications", None);
                    item.setEnabled(false);
                    menu.addItem(&item);
                } else {
                    for (bundle_id, name) in applications {
                        let icon = application_icon(bundle_id.as_deref(), &name);
                        if let Some(icon) = icon.as_ref() {
                            group_icons.push(icon.clone());
                        }
                        let item = menu_item(mtm, &name, icon);
                        menu.addItem(&item);
                    }
                }
                if !group_icons.is_empty() {
                    status_groups.push((index, group_icons));
                }
            }

            if let Some(button) = self.ivars().status_item.button(mtm) {
                self.ivars().view.set_groups(status_groups);
                button.addSubview(&*self.ivars().view);
            }
            let width = self.ivars().view.frame().size.width;
            self.ivars().status_item.setLength(width);
            self.ivars().status_item.setMenu(Some(&menu));
            *self.ivars().menu.borrow_mut() = Some(menu);
        }
    }
);

fn separator() -> Retained<NSMenuItem> {
    unsafe { msg_send![NSMenuItem::class(), separatorItem] }
}

fn menu_item(
    mtm: MainThreadMarker,
    title: &str,
    icon: Option<Retained<objc2_app_kit::NSImage>>,
) -> Retained<NSMenuItem> {
    let title = NSString::from_str(title);
    let key = NSString::from_str("");
    let item: Retained<NSMenuItem> = unsafe {
        msg_send![NSMenuItem::alloc(mtm), initWithTitle: &*title, action: None::<objc2::runtime::Sel>, keyEquivalent: &*key]
    };
    if let Some(icon) = icon {
        unsafe {
            let _: () = msg_send![&*item, setImage: &*icon];
        }
    }
    item
}

fn application_icon(
    bundle_id: Option<&str>,
    app_name: &str,
) -> Option<Retained<objc2_app_kit::NSImage>> {
    if let Some(bundle_id) = bundle_id {
        let bundle_id = NSString::from_str(bundle_id);
        let applications: Option<Retained<AnyObject>> = unsafe {
            msg_send![NSRunningApplication::class(), runningApplicationsWithBundleIdentifier: &*bundle_id]
        };
        if let Some(application) = applications.and_then(|applications| unsafe {
            let application: Option<Retained<NSRunningApplication>> =
                msg_send![&*applications, firstObject];
            application
        }) {
            if let Some(icon) = unsafe { msg_send![&*application, icon] } {
                return Some(icon);
            }
        }
    }

    let app_name = NSString::from_str(app_name);
    let workspace = NSWorkspace::sharedWorkspace();
    let path: Option<Retained<NSString>> =
        unsafe { msg_send![&*workspace, fullPathForApplication: &*app_name] };
    let path = path?;
    unsafe { msg_send![&*workspace, iconForFile: &*path] }
}

fn main() {
    let mtm = MainThreadMarker::new().expect("must run on the macOS main thread");
    let application = NSApplication::sharedApplication(mtm);
    let _ = application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    application.finishLaunching();
    NSApplication::load();

    let status_bar = NSStatusBar::systemStatusBar();
    let status_item = status_bar.statusItemWithLength(NSVariableStatusItemLength);
    let view = mtm.alloc().set_ivars(IndicatorViewIvars {
        groups: RefCell::new(Vec::new()),
    });
    let view: Retained<IndicatorView> = unsafe {
        msg_send![super(view), initWithFrame: CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(0.0, 17.0))]
    };
    let indicator = mtm.alloc().set_ivars(IndicatorIvars {
        status_item,
        view,
        menu: RefCell::new(None),
    });
    let indicator: Retained<Indicator> = unsafe { msg_send![super(indicator), init] };
    unsafe {
        let _: () = msg_send![&*indicator, refresh: None::<&AnyObject>];
    }

    let pointer = Retained::as_ptr(&indicator) as usize;
    thread::spawn(move || {
        let Ok(client) = RiftMachClient::connect() else {
            return;
        };
        let Ok(subscription) = client.subscribe(EventKind::All) else {
            return;
        };
        loop {
            if subscription.recv_event().is_err() {
                break;
            }
            // The object is intentionally kept alive for the lifetime of NSApp.
            let indicator = unsafe { &*(pointer as *const Indicator) };
            unsafe {
                let _: () = msg_send![indicator,
                    performSelectorOnMainThread: sel!(refresh:),
                    withObject: None::<&AnyObject>,
                    waitUntilDone: false
                ];
            }
        }
    });

    std::mem::forget(indicator);
    application.run();
}

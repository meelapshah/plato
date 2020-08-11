use std::fs::{self, File};
use std::path::PathBuf;
use rand_core::RngCore;
use fxhash::FxHashMap;
use chrono::Local;
use walkdir::WalkDir;
use globset::Glob;
use anyhow::Error;
use crate::device::CURRENT_DEVICE;
use crate::geom::{Point, Rectangle, CornerSpec, halves};
use crate::input::{DeviceEvent, FingerStatus, ButtonCode, ButtonStatus};
use crate::view::icon::Icon;
use crate::view::notification::Notification;
use crate::view::menu::{Menu, MenuKind};
use crate::view::common::{locate_by_id};
use crate::view::{View, Event, Hub, Bus, EntryKind, EntryId, ViewId};
use crate::framebuffer::{Framebuffer, UpdateMode, Pixmap};
use crate::settings::{ImportSettings, Pen};
use crate::helpers::IsHidden;
use crate::font::Fonts;
use crate::unit::scale_by_dpi;
use crate::view::label;
use crate::color::{BLACK, WHITE};
use crate::app::Context;
use crate::view::top_bar::TopBar;
use crate::view::{SMALL_BAR_HEIGHT, BIG_BAR_HEIGHT, THICKNESS_MEDIUM};
use crate::view::common::{toggle_main_menu, toggle_battery_menu, toggle_clock_menu};
use tokio::prelude::*;
use std::sync::{Mutex, Arc};
use mango_client::mango::*;
use mango_client::opds::OpdsClient;
use std::thread;

#[derive(Debug, Clone)]
pub enum MangoEvent {
    GotLibrary(Library),
}

pub struct Mango {
    children: Vec<Box<dyn View>>,
    rect: Rectangle,
    client: Arc<MangoClient>,
}

impl Mango {
    pub fn new(rect: Rectangle, hub: &Hub, context: &mut Context) -> Mango {
        println!("Rect: {:?}", rect);
        let mut children: Vec<Box<dyn View>> = Vec::new();
        
        let dpi = CURRENT_DEVICE.dpi;
        let (small_height, big_height) = (scale_by_dpi(SMALL_BAR_HEIGHT, dpi) as i32,
                                          scale_by_dpi(BIG_BAR_HEIGHT, dpi) as i32);
        let thickness = scale_by_dpi(THICKNESS_MEDIUM, dpi) as i32;
        let (small_thickness, big_thickness) = halves(thickness);
        let side = small_height;


        let top_bar = TopBar::new(rect![rect.min.x, rect.min.y,
                                rect.max.x, rect.min.y + side - small_thickness],
                            Event::Back,
                            "Fetching library...".to_string(),
                            context);
        children.push(Box::new(top_bar) as Box<dyn View>);

        let test_label = label::Label::new(
            rect![0, rect.min.y + side - small_thickness, rect.max.x, rect.max.y],
            String::from("..."),
            super::Align::Left(10));
        children.push(Box::new(test_label));

        
        hub.send(Event::Focus(Some(ViewId::Mango))).ok();
        hub.send(Event::Render(rect, UpdateMode::Full)).ok();

        let app = Mango {
            children,
            rect,
            client: Arc::new(MangoClient::new(
                OpdsClient::new(
                    &context.settings.mango.url,
                    &context.settings.mango.username,
                    &context.settings.mango.password)
            )),
        };
        app.resolve_library(hub.clone());
        app
    }

    fn resolve_library(&self, hub: Hub) {
        let client = self.client.clone();
        thread::spawn(move || {
            /*thread::sleep_ms(3000);
            let lib = Library {
                title: "Bla".to_owned(),
                entries: Vec::new(),
            };*/
            let lib = tokio::runtime::Runtime::new().unwrap().block_on((*client).library()).unwrap();
            hub.send(Event::Mango(MangoEvent::GotLibrary(lib))).unwrap();
        });
    }

    fn top_bar(&mut self) -> &mut TopBar {
        if let Some(top_bar) = self.child_mut(0).downcast_mut::<TopBar>() {
            top_bar
        }else {
            panic!("Top bar not found!")
        }
    }

    fn test_label(&mut self) -> &mut label::Label {
        if let Some(test_label) = self.child_mut(1).downcast_mut::<label::Label>() {
            test_label
        }else {
            panic!("Top bar not found!")
        }
    }
}

impl View for Mango {
    fn handle_event(&mut self, evt: &Event, hub: &Hub, bus: &mut Bus, context: &mut Context) -> bool {
        println!("Got event: {:?}", evt);
        let mut consumed = match *evt {
            Event::Device(device_event) => match device_event {
                DeviceEvent::Button { ref code, ref status, .. } => 
                if let ButtonStatus::Released = status {
                    match code {
                        ButtonCode::Home => {
                            hub.send(Event::Back).ok();
                            true
                        },
                        _ => false
                    }
                }else {
                    false
                },
                _ => false
            },
            Event::ToggleNear(ViewId::MainMenu, rect) => {
                toggle_main_menu(self, rect, None, hub, context);
                true
            },
            Event::ToggleNear(ViewId::BatteryMenu, rect) => {
                toggle_battery_menu(self, rect, None, hub, context);
                true
            },
            Event::ToggleNear(ViewId::ClockMenu, rect) => {
                toggle_clock_menu(self, rect, None, hub, context);
                true
            },
            Event::Mango(ref mango_event) => match mango_event {
                MangoEvent::GotLibrary(ref lib) => {
                    self.top_bar().update_title_label(&lib.title, hub);
                    let mut text = String::new();
                    for ref entry in lib.entries.iter() {
                        text.push_str(" - ");
                        text.push_str(&entry.title);
                        text.push('\n');
                    }
                    self.test_label().update(&text, hub);
                    hub.send(Event::Render(self.rect, UpdateMode::Full)).ok();
                    true
                },
                _ => false
            },
            _ => false
        };

        if ! consumed {
            for child in self.children_mut() {
                consumed = child.handle_event(evt, hub, bus, context);
                if consumed {
                    break;
                }
            }
        }

        consumed
    }
    fn render(&self, fb: &mut dyn Framebuffer, rect: Rectangle, fonts: &mut Fonts) {
        for ref child in self.children().iter() {
            child.render(fb, rect, fonts)
        }
    }
    fn rect(&self) -> &Rectangle {
        &self.rect
    }
    fn rect_mut(&mut self) -> &mut Rectangle {
        &mut self.rect
    }
    fn children(&self) -> &Vec<Box<dyn View>> {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn View>> {
        &mut self.children
    }

}
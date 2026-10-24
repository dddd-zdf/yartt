use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use log::{debug, warn};
use loole::Sender;
use windows::core::{h, IInspectable};
use windows::Foundation::Size;
use windows::UI::Color;
use windows_ext::UI::Xaml::Controls::Control;
use windows_ext::UI::Xaml::Controls::Primitives::FlyoutPlacementMode;
use windows_ext::UI::Xaml::Media::{AcrylicBackgroundSource, AcrylicBrush, SolidColorBrush};
use windows_ext::UI::Xaml::{TextAlignment, VerticalAlignment};

use crate::backend::MonitorControllerProxy;
use crate::monitors::MonitorPath;
use crate::theme::ColorSet;
use crate::ui::container::{Grid, GridSize, StackPanel};
use crate::ui::controls::{AppBarButton, Flyout, FontIcon, Slider, TextBlock};
use crate::ui::FontWeight;
use crate::utils::extensions::ChannelExt;
use crate::utils::ordered_map::{OrderedMap, SortKeyExtract};
use crate::windowing::{Window, WindowBuilder, WindowLevel};
use crate::{cloned, hformat, log_assert, CustomEvent, Result};

pub struct ProxyWindow {
    window: Window,
    content: TextBlock
}

impl ProxyWindow {
    pub fn new() -> Result<Self> {
        let window = WindowBuilder::default()
            .with_position(0, 0)
            .with_size(10, 10)
            .with_hidden(true)
            .build()?;
        let content = TextBlock::with_text("This should be invisible!")?;
        window.set_content(&content)?;
        Ok(Self { window, content })
    }

    pub fn activate(&self) -> bool {
        self.window
            .set_window_pos(Some(WindowLevel::AlwaysOnTop), Some((0, 0)), None, Some(true));
        self.window.focus();
        self.window.set_foreground()
    }

    pub fn deactivate(&self) {
        self.window.set_window_pos(None, None, None, Some(false))
    }

    pub fn dpi(&self) -> f32 {
        self.window.dpi()
    }
}

pub struct BrightnessFlyout {
    flyout: Flyout,
    background: AcrylicBrush,
    container: StackPanel,
    link_button: AppBarButton,
    linked: Cell<bool>,
    /// One slider for every monitor, shown instead of the per-monitor ones while linked
    linked_row: BrightnessRow,
    /// Set while the flyout moves the linked slider itself, so that isn't sent to the monitors
    linked_suppress: Arc<AtomicBool>,

    monitor_panel: StackPanel,
    monitor_controls: OrderedMap<MonitorPath, MonitorEntry>
}

impl BrightnessFlyout {
    const WIDTH: f64 = 400.0;

    pub fn new(sender: Sender<CustomEvent>, colors: &ColorSet, linked: bool) -> Result<Self> {
        let link_button = AppBarButton::new()?
            .with_icon(&FontIcon::new('\u{E71B}')?)?
            .with_label(Self::link_label(linked))?
            .with_click_handler(cloned!([sender] move || {
                sender.send_ignore(CustomEvent::ToggleLinked);
                Ok(())
            }))?;

        let settings = AppBarButton::new()?
            .with_icon(&FontIcon::new('\u{E713}')?)?
            .with_label("Settings")?
            //.with_enabled(false)?;
            .with_click_handler(cloned!([sender] move|| {
                sender
                    .send(CustomEvent::OpenSettings)
                    .unwrap_or_else(|_| log::warn!("Failed to send settings event"));
                Ok(())
            }))?;

        let refresh = AppBarButton::new()?
            .with_icon(&FontIcon::new('\u{E72C}')?)?
            .with_label("Refresh")?
            .with_click_handler(cloned!([sender] move || {
                sender
                    .send(CustomEvent::Refresh)
                    .unwrap_or_else(|_| log::warn!("Failed to send refresh event"));
                Ok(())
            }))?;

        // Create a new stack panel for the bottom bar
        let bottom_bar = Grid::new()?
            //.with_padding(20.0)?
            .with_column_widths([GridSize::Fraction(1.0), GridSize::Auto])?
            .with_background(&SolidColorBrush::CreateInstanceWithColor(Color { R: 0, G: 0, B: 0, A: 70 })?)?
            .with_child(
                &TextBlock::with_text("Adjust Brightness")?
                    .with_font_size(15.0)?
                    .with_vertical_alignment(VerticalAlignment::Center)?
                    .with_padding((20.0, 0.0, 0.0, 0.0))?,
                0,
                0
            )?
            .with_child(
                &StackPanel::horizontal()?
                    .with_child(&link_button)?
                    .with_child(&refresh)?
                    .with_child(&settings)?,
                0,
                1
            )?;

        let monitor_panel = StackPanel::vertical()?
            .with_spacing(20.0)?
            .with_padding((20.0, 14.0))?;

        let container = StackPanel::vertical()?
            .with_theme(colors.theme)?
            .with_width(Self::WIDTH)?
            .with_child(
                &Grid::new()?
                    .with_row_heights([GridSize::Auto, GridSize::Fraction(1.0), GridSize::Auto])?
                    .with_child(&monitor_panel, 0, 0)?
                    .with_child(&bottom_bar, 2, 0)?
            )?;

        let background = {
            let brush = AcrylicBrush::new()?;
            brush.SetBackgroundSource(AcrylicBackgroundSource::HostBackdrop)?;
            brush.SetFallbackColor(colors.fallback)?;
            brush.SetTintColor(colors.tint)?;
            brush.SetTintOpacity(colors.opacity)?;
            brush
        };

        let flyout = Flyout::new(&container)?
            .with_style(|style| {
                style
                    .with_setter(&Control::BackgroundProperty()?, &background)?
                    .with_setter(&Control::CornerRadiusProperty()?, &IInspectable::try_from(h!("10.0"))?)?
                    .with_setter(&Control::PaddingProperty()?, &IInspectable::try_from(h!("0.0"))?)
            })?
            .with_closed_handler(cloned!([sender] move || {
                sender.filter_send_ignore(Some(CustomEvent::FocusLost));
                Ok(())
            }))?;

        let linked_suppress = Arc::new(AtomicBool::new(false));
        let linked_row = BrightnessRow::create('\u{E71B}', "All monitors", {
            let suppress = linked_suppress.clone();
            move |value| {
                if !suppress.load(Ordering::Relaxed) {
                    sender.send_ignore(CustomEvent::LinkedBrightnessChanged(value));
                }
            }
        })?;

        Ok(Self {
            flyout,
            background,
            container,
            link_button,
            linked: Cell::new(linked),
            linked_row,
            linked_suppress,
            monitor_panel,
            monitor_controls: OrderedMap::new()
        })
    }

    fn link_label(linked: bool) -> &'static str {
        if linked {
            "Linked"
        } else {
            "Unlinked"
        }
    }

    /// Turning linking on brings every monitor to the level of the first one.
    pub fn set_linked(&self, linked: bool) -> Result<()> {
        self.linked.set(linked);
        self.link_button.set_label(Self::link_label(linked))?;
        if linked {
            if let Some(first) = self.monitor_controls.values().next() {
                let value = first.row.slider.get_value()? as u32;
                self.set_linked_slider(value)?;
                self.set_all(value)?;
            }
        }
        self.repopulate_monitor_list()
    }

    /// Sends one level to every monitor, from the linked slider.
    pub fn set_all(&self, value: u32) -> Result<()> {
        // The per-monitor sliders are hidden while linked, but they still pass the value on
        self.monitor_controls
            .values()
            .try_for_each(|entry| entry.set_brightness(value))
    }

    fn set_linked_slider(&self, value: u32) -> Result<()> {
        self.linked_suppress.store(true, Ordering::Relaxed);
        let result = self.linked_row.slider.set_value(value as f64);
        self.linked_suppress.store(false, Ordering::Relaxed);
        result
    }

    pub fn update_theme(&self, colors: &ColorSet) -> Result<()> {
        self.container.set_theme(colors.theme)?;
        self.background.SetFallbackColor(colors.fallback)?;
        self.background.SetTintColor(colors.tint)?;
        self.background.SetTintOpacity(colors.opacity)?;
        Ok(())
    }

    pub fn is_open(&self) -> bool {
        self.flyout
            .is_open()
            .map_err(|e| warn!("Failed to check if flyout is open: {}", e))
            .unwrap_or(false)
    }

    pub fn close(&self) {
        self.flyout
            .close()
            .unwrap_or_else(|e| warn!("Failed to close flyout: {}", e));
    }

    pub fn show(&self, proxy_window: &ProxyWindow, x: f32, y: f32) {
        self.flyout
            .show_at(&proxy_window.content, x, y, FlyoutPlacementMode::LeftEdgeAlignedBottom)
            .unwrap_or_else(|e| warn!("Failed to show flyout: {}", e));
    }

    pub fn size(&self) -> Size {
        self.container.measure().unwrap_or_else(|e| {
            debug!("Failed to get measured size of flyout: {}", e);
            // Make a guess
            Size {
                Width: Self::WIDTH as f32,
                Height: 62.0 + 86.0 * self.monitor_controls.len() as f32
            }
        })
    }

    fn repopulate_monitor_list(&self) -> Result<()> {
        self.monitor_panel.clear_children()?;
        if self.linked.get() && !self.monitor_controls.is_empty() {
            self.monitor_panel.add_child(&self.linked_row.ui)?;
        } else {
            for monitor in self.monitor_controls.values() {
                self.monitor_panel.add_child(monitor.ui())?;
            }
        }
        Ok(())
    }

    pub fn register_monitor(&mut self, name: String, path: MonitorPath, position: i32, proxy: MonitorControllerProxy) -> Result<()> {
        log_assert!(self
            .monitor_controls
            .insert(path.clone(), MonitorEntry::create(name, path, position, proxy)?)
            .is_none());
        self.repopulate_monitor_list()?;
        Ok(())
    }

    pub fn unregister_monitor(&mut self, path: &MonitorPath) -> Result<()> {
        log_assert!(self.monitor_controls.remove(path).is_some());
        self.repopulate_monitor_list()?;
        Ok(())
    }

    pub fn clear_monitors(&mut self) -> Result<()> {
        self.monitor_controls.clear();
        self.repopulate_monitor_list()?;
        Ok(())
    }

    pub fn rename_monitor(&self, path: &MonitorPath, name: String) -> Result<()> {
        match self.monitor_controls.get(path) {
            None => warn!("Monitor is not registered: {:?}", path),
            Some(entry) => entry.set_name(&name)?
        }
        Ok(())
    }

    pub fn update_brightness(&self, path: MonitorPath, new_brightness: u32) -> Result<()> {
        match self.monitor_controls.iter().position(|(p, _)| *p == path) {
            None => warn!("Monitor is not registered: {:?}", path),
            Some(index) => {
                self.monitor_controls.get(&path).unwrap().set_brightness(new_brightness)?;
                // The linked slider follows the first monitor
                if self.linked.get() && index == 0 {
                    self.set_linked_slider(new_brightness)?;
                }
            }
        }
        Ok(())
    }

    pub fn change_brightness(&self, delta: i32) -> Result<()> {
        if self.linked.get() {
            let slider = &self.linked_row.slider;
            return slider.set_value(slider.get_value()? + delta as f64);
        }
        for entry in self.monitor_controls.values() {
            entry.change_brightness(delta)?;
        }
        Ok(())
    }
}

/// An icon and name above a brightness slider with its value.
struct BrightnessRow {
    ui: StackPanel,
    slider: Slider,
    name_label: TextBlock
}

impl BrightnessRow {
    fn create(icon: char, name: &str, mut on_change: impl FnMut(u32) + Send + 'static) -> Result<Self> {
        let label = TextBlock::new()?
            .with_vertical_alignment(VerticalAlignment::Center)?
            .with_text_alignment(TextAlignment::Center)?
            .with_font_size(20.0)?
            .with_font_weight(FontWeight::Medium)?;

        let name_label = TextBlock::with_text(name)?.with_font_size(20.0)?;

        let slider = Slider::new()?
            .with_vertical_alignment(VerticalAlignment::Center)?
            .with_value(0.0)?
            .with_mouse_scrollable()?
            .with_value_changed_handler(cloned!([label] move |args| {
                let new = args.NewValue()? as u32;
                label.set_text(hformat!("{}", new))?;
                on_change(new);
                Ok(())
            }))?;

        label.set_text(hformat!("{}", slider.get_value()?))?;

        let ui = StackPanel::vertical()?
            .with_spacing(4.0)?
            .with_child(
                &StackPanel::horizontal()?
                    .with_spacing(8.0)?
                    .with_child(&FontIcon::new(icon)?.with_font_weight(FontWeight::Medium)?)?
                    .with_child(&name_label)?
            )?
            .with_child(
                &Grid::new()?
                    .with_column_widths([GridSize::Fraction(1.0), GridSize::Pixel(40.0)])?
                    .with_column_spacing(8.0)?
                    .with_child(&slider, 0, 0)?
                    .with_child(&label, 0, 1)?
            )?;

        Ok(Self { ui, slider, name_label })
    }
}

struct MonitorEntry {
    position: i32,
    row: BrightnessRow
}

impl MonitorEntry {
    fn create(name: String, path: MonitorPath, position: i32, proxy: MonitorControllerProxy) -> Result<Self> {
        let row = BrightnessRow::create('\u{E7f4}', &name, move |value| proxy.set_brightness(path.clone(), value))?;
        Ok(Self { position, row })
    }

    pub fn set_name(&self, name: &str) -> Result<()> {
        self.row.name_label.set_text(name)
    }

    pub fn set_brightness(&self, value: u32) -> Result<()> {
        self.row.slider.set_value(value as f64)
    }

    pub fn change_brightness(&self, value: i32) -> Result<()> {
        self.row.slider.set_value(self.row.slider.get_value()? + value as f64)
    }

    pub fn ui(&self) -> &StackPanel {
        &self.row.ui
    }
}

impl SortKeyExtract for MonitorEntry {
    type Key = i32;

    fn sort_key(&self) -> i32 {
        self.position
    }
}

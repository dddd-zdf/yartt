use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use log::{debug, warn};
use loole::Sender;
use windows::core::{h, IInspectable};
use windows::Foundation::Size;
use windows::UI::Color;
use windows_ext::UI::Xaml::Controls::Control;
use windows_ext::UI::Xaml::Controls::Primitives::FlyoutPlacementMode;
use windows_ext::UI::Xaml::Media::{AcrylicBackgroundSource, AcrylicBrush, SolidColorBrush};
use windows_ext::UI::Xaml::{TextAlignment, VerticalAlignment};

use crate::backend::{Feature, FeatureValue, MonitorControllerProxy};
use crate::capabilities::input_name;
use crate::monitors::MonitorPath;
use crate::theme::ColorSet;
use crate::ui::container::{Grid, GridSize, StackPanel};
use crate::ui::controls::{AppBarButton, ComboBox, Flyout, FontIcon, Slider, TextBlock};
use crate::ui::FontWeight;
use crate::utils::extensions::{ChannelExt, MutexExt};
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
    shown: Cell<Shown>,
    /// One slider for every monitor, shown instead of the per-monitor ones while linked
    linked_row: LinkedRow,
    /// Set while the flyout moves the linked slider itself, so that isn't sent to the monitors
    linked_suppress: Arc<AtomicBool>,

    monitor_panel: StackPanel,
    monitor_controls: OrderedMap<MonitorPath, MonitorEntry>
}

impl BrightnessFlyout {
    const WIDTH: f64 = 400.0;

    pub fn new(sender: Sender<CustomEvent>, colors: &ColorSet, linked: bool, shown: Shown) -> Result<Self> {
        let link_button = AppBarButton::new()?
            .with_icon(&FontIcon::new('\u{E71B}')?)?
            .with_label(Self::link_label(linked))?
            .with_click_handler(cloned!([sender] move || {
                sender.send_ignore(CustomEvent::ToggleLinked);
                Ok(())
            }))?;

        let screen_off = AppBarButton::new()?
            .with_icon(&FontIcon::new('\u{E7E8}')?)?
            .with_label("Screen off")?
            .with_click_handler(cloned!([sender] move || {
                sender.send_ignore(CustomEvent::ScreenOff);
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
                    .with_child(&screen_off)?
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
        let linked_row = LinkedRow::create({
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
            shown: Cell::new(shown),
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
                let value = first.brightness.slider.get_value()? as u32;
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
        let linked = self.linked.get() && !self.monitor_controls.is_empty();
        if linked {
            self.monitor_panel.add_child(&self.linked_row.ui)?;
        }
        for monitor in self.monitor_controls.values() {
            monitor.layout(linked, self.shown.get())?;
            // While linked, a monitor with nothing but brightness has nothing left to show
            if !linked || monitor.has_features(self.shown.get()) {
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

    pub fn set_shown(&self, shown: Shown) -> Result<()> {
        self.shown.set(shown);
        self.repopulate_monitor_list()
    }

    pub fn update_feature(&self, path: &MonitorPath, feature: Feature, value: Option<FeatureValue>) -> Result<()> {
        match self.monitor_controls.get(path) {
            None => warn!("Monitor is not registered: {:?}", path),
            Some(entry) => {
                if entry.update_feature(feature, value)? {
                    self.repopulate_monitor_list()?;
                }
            }
        }
        Ok(())
    }

    pub fn set_inputs(&self, path: &MonitorPath, inputs: Vec<u32>) -> Result<()> {
        match self.monitor_controls.get(path) {
            None => warn!("Monitor is not registered: {:?}", path),
            Some(entry) => {
                if entry.set_inputs(inputs)? {
                    self.repopulate_monitor_list()?;
                }
            }
        }
        Ok(())
    }

    /// Current levels for the tray tooltip: one line while linked, else one per monitor.
    pub fn brightness_summary(&self) -> Result<String> {
        if self.monitor_controls.is_empty() {
            return Ok("Change Brightness".into());
        }
        if self.linked.get() {
            return Ok(format!("Brightness: {}%", self.linked_row.slider.get_value()?));
        }
        let lines = self
            .monitor_controls
            .values()
            .map(|entry| Ok(format!("{}: {}%", entry.name_label.text()?, entry.brightness.slider.get_value()?)))
            .collect::<Result<Vec<_>>>()?;
        Ok(lines.join("
"))
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

/// Which optional rows the settings turned on.
#[derive(Debug, Copy, Clone, Default)]
pub struct Shown {
    pub volume: bool,
    pub input: bool
}

/// An icon and a name, the title line of a monitor or of the linked slider.
fn header(icon: char, name: &str) -> Result<(StackPanel, TextBlock)> {
    let name_label = TextBlock::with_text(name)?.with_font_size(20.0)?;
    let ui = StackPanel::horizontal()?
        .with_spacing(8.0)?
        .with_child(&FontIcon::new(icon)?.with_font_weight(FontWeight::Medium)?)?
        .with_child(&name_label)?;
    Ok((ui, name_label))
}

/// A slider with its value next to it, optionally led by an icon.
struct SliderRow {
    ui: Grid,
    slider: Slider
}

impl SliderRow {
    fn create(icon: Option<char>, font_size: f64, mut on_change: impl FnMut(u32) + Send + 'static) -> Result<Self> {
        let label = TextBlock::new()?
            .with_vertical_alignment(VerticalAlignment::Center)?
            .with_text_alignment(TextAlignment::Center)?
            .with_font_size(font_size)?
            .with_font_weight(FontWeight::Medium)?;

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

        let ui = match icon {
            Some(icon) => Grid::new()?
                .with_column_widths([GridSize::Pixel(28.0), GridSize::Fraction(1.0), GridSize::Pixel(40.0)])?
                .with_column_spacing(8.0)?
                .with_child(&FontIcon::new(icon)?.with_vertical_alignment(VerticalAlignment::Center)?, 0, 0)?
                .with_child(&slider, 0, 1)?
                .with_child(&label, 0, 2)?,
            None => Grid::new()?
                .with_column_widths([GridSize::Fraction(1.0), GridSize::Pixel(40.0)])?
                .with_column_spacing(8.0)?
                .with_child(&slider, 0, 0)?
                .with_child(&label, 0, 1)?
        };

        Ok(Self { ui, slider })
    }
}

/// The "All monitors" slider shown while linked.
struct LinkedRow {
    ui: StackPanel,
    slider: Slider
}

impl LinkedRow {
    fn create(on_change: impl FnMut(u32) + Send + 'static) -> Result<Self> {
        let (title, _) = header('\u{E71B}', "All monitors")?;
        let row = SliderRow::create(None, 20.0, on_change)?;
        let ui = StackPanel::vertical()?
            .with_spacing(4.0)?
            .with_child(&title)?
            .with_child(&row.ui)?;
        Ok(Self { ui, slider: row.slider })
    }
}

struct MonitorEntry {
    position: i32,
    ui: StackPanel,
    title: StackPanel,
    name_label: TextBlock,
    brightness: SliderRow,
    volume: SliderRow,
    has_volume: Cell<bool>,
    input_row: Grid,
    input_box: ComboBox,
    /// Input values the monitor lists, in the order shown in `input_box`
    inputs: Arc<Mutex<Vec<u32>>>,
    current_input: Cell<Option<u32>>
}

impl MonitorEntry {
    fn create(name: String, path: MonitorPath, position: i32, proxy: MonitorControllerProxy) -> Result<Self> {
        let (title, name_label) = header('\u{E7f4}', &name)?;

        let brightness = SliderRow::create(None, 20.0, cloned!([proxy, path] move |value| proxy.set_brightness(path.clone(), value)))?;
        let volume = SliderRow::create(
            Some('\u{E767}'),
            15.0,
            cloned!([proxy, path] move |value| proxy.set_feature(path.clone(), Feature::Volume, value))
        )?;

        // Filled in once the monitor's input list arrives; the handler maps an index back to its value
        let inputs = Arc::new(Mutex::new(Vec::<u32>::new()));
        let input_box = ComboBox::new()?
            .with_min_width(160.0)?
            .with_placeholder("Input")?
            .with_vertical_alignment(VerticalAlignment::Center)?
            .with_selection_changed_handler(cloned!([inputs] move |index| {
                let value = usize::try_from(index)
                    .ok()
                    .and_then(|i| inputs.lock_no_poison().get(i).copied());
                if let Some(value) = value {
                    proxy.set_feature(path.clone(), Feature::Input, value);
                }
                Ok(())
            }))?;
        let input_row = Grid::new()?
            .with_column_widths([GridSize::Pixel(28.0), GridSize::Fraction(1.0)])?
            .with_column_spacing(8.0)?
            .with_child(&FontIcon::new('\u{E8B9}')?.with_vertical_alignment(VerticalAlignment::Center)?, 0, 0)?
            .with_child(&input_box, 0, 1)?;

        Ok(Self {
            position,
            ui: StackPanel::vertical()?.with_spacing(4.0)?,
            title,
            name_label,
            brightness,
            volume,
            has_volume: Cell::new(false),
            input_row,
            input_box,
            inputs,
            current_input: Cell::new(None)
        })
    }

    /// Lays out the rows this monitor has. While linked the brightness slider is left out,
    /// since the shared one replaces it.
    fn layout(&self, linked: bool, shown: Shown) -> Result<()> {
        self.ui.clear_children()?;
        self.ui.add_child(&self.title)?;
        if !linked {
            self.ui.add_child(&self.brightness.ui)?;
        }
        if shown.volume && self.has_volume.get() {
            self.ui.add_child(&self.volume.ui)?;
        }
        if shown.input && !self.inputs.lock_no_poison().is_empty() {
            self.ui.add_child(&self.input_row)?;
        }
        Ok(())
    }

    /// Whether anything besides brightness can be shown.
    fn has_features(&self, shown: Shown) -> bool {
        (shown.volume && self.has_volume.get()) || (shown.input && !self.inputs.lock_no_poison().is_empty())
    }

    pub fn set_name(&self, name: &str) -> Result<()> {
        self.name_label.set_text(name)
    }

    pub fn set_brightness(&self, value: u32) -> Result<()> {
        self.brightness.slider.set_value(value as f64)
    }

    pub fn change_brightness(&self, value: i32) -> Result<()> {
        self.brightness
            .slider
            .set_value(self.brightness.slider.get_value()? + value as f64)
    }

    /// Returns whether the set of rows changed.
    fn update_feature(&self, feature: Feature, value: Option<FeatureValue>) -> Result<bool> {
        match feature {
            Feature::Volume => {
                let changed = self.has_volume.replace(value.is_some()) != value.is_some();
                if let Some(value) = value {
                    self.volume.slider.set_value(value.current as f64)?;
                }
                Ok(changed)
            }
            Feature::Input => {
                self.current_input.set(value.map(|v| v.current));
                self.select_current_input()?;
                Ok(false)
            }
            Feature::Power => Ok(false)
        }
    }

    /// Returns whether the set of rows changed.
    fn set_inputs(&self, inputs: Vec<u32>) -> Result<bool> {
        let names: Vec<String> = inputs.iter().map(|&v| input_name(v)).collect();
        let was_empty = std::mem::replace(&mut *self.inputs.lock_no_poison(), inputs).is_empty();
        // The lock must be released here: filling the box fires the selection handler, which takes it
        self.input_box.set_items(names)?;
        self.select_current_input()?;
        Ok(was_empty != self.inputs.lock_no_poison().is_empty())
    }

    fn select_current_input(&self) -> Result<()> {
        // Some monitors report a current input that isn't in their own list (the LG says 0)
        let index = self
            .current_input
            .get()
            .and_then(|current| self.inputs.lock_no_poison().iter().position(|&v| v == current));
        self.input_box
            .set_selected_index(index.map_or(-1, |i| i as i32))
    }

    pub fn ui(&self) -> &StackPanel {
        &self.ui
    }
}

impl SortKeyExtract for MonitorEntry {
    type Key = i32;

    fn sort_key(&self) -> i32 {
        self.position
    }
}

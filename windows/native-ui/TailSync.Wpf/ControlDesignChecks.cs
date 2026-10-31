using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Media;

namespace TailSync.Wpf;

internal static class ControlDesignChecks
{
    // Runs in the existing isolated regression process; never writes the user's
    // appearance or connection preferences. Captures are WPF render-tree images.
    internal static async Task Run(string directory, Action<bool, string> check)
    {
        foreach (var kind in VectorIcon.Symbols.Keys)
        {
            var (font, index) = VectorIcon.Resolve(kind);
            check(index != 0 && font.AdvanceWidths[index] > 0, $"Unicode icon {kind} resolves to an installed font glyph");
        }
        foreach (var mode in new[] { "light", "dark" })
        {
            ThemeService.Apply(mode, "canvas");
            var root = new StackPanel { Margin = new(24) };
            root.Children.Add(Ui.Text("控件与图标 · " + mode, 20));
            var primary = Ui.Button("确认连接", style: "PrimaryButton", icon: "check");
            var secondary = Ui.Button("复制邀请", icon: "copy");
            var disabled = Ui.Button("暂不可用", icon: "link"); disabled.IsEnabled = false;
            root.Children.Add(Ui.Stack(Orientation.Horizontal, 10, primary, secondary, disabled, Ui.IconButton("refresh", "刷新", () => { })));
            var input = new TextBox { Text = "TailSync · 清晰的中文输入", Margin = new(0, 16, 0, 12) }; root.Children.Add(input);
            var toggle = new ToggleButton { IsChecked = true }; toggle.SetResourceReference(FrameworkElement.StyleProperty, "MacSwitch");
            var combo = new ComboBox { Width = 160, ItemsSource = new[] { "自动连接", "局域网", "Iroh" }, SelectedIndex = 0 };
            root.Children.Add(Ui.Stack(Orientation.Horizontal, 16, combo, toggle, new CheckBox { Content = "提高对比度", IsChecked = true }));
            root.Children.Add(new Slider { Minimum = 0, Maximum = 100, Value = 45, Margin = new(0, 12, 0, 12) });
            var date = new DatePicker { SelectedDate = new DateTime(2026, 10, 30), Width = 180, HorizontalAlignment = HorizontalAlignment.Left }; root.Children.Add(date);
            var shortcutSamples = new StackPanel { Margin = new(0, 16, 0, 0) };
            foreach (var (title, shortcut) in new[] { ("同步快捷键", "Alt+Quote"), ("历史记录快捷键", "Alt+Slash"), ("组合键", "CommandOrControl+Alt+Shift+KeyS"), ("Windows 键", "Super+ArrowLeft"), ("小键盘", "Control+NumpadAdd") })
            {
                var copy = Ui.Text(title, 13, "Secondary"); copy.Width = 140;
                var row = Ui.Stack(Orientation.Horizontal, 12, copy, ShortcutDisplayFormatter.Keycaps(shortcut)); row.Margin = new(0, 0, 0, 6); shortcutSamples.Children.Add(row);
            }
            shortcutSamples.Children.Add(ShortcutDisplayFormatter.Keycaps("Alt+Shift+Slash", large: true)); root.Children.Add(shortcutSamples);
            var symbols = new WrapPanel { Margin = new(0, 16, 0, 0) };
            foreach (var kind in VectorIcon.Symbols.Keys)
            {
                var item = Ui.Stack(Orientation.Vertical, 5, new VectorIcon { Kind = kind, Width = 20, Height = 20, Stroke = Ui.Brush("Primary") }, Ui.Text(kind, 10, "Secondary"));
                item.Width = 65; item.Height = 54; symbols.Children.Add(item);
            }
            root.Children.Add(symbols);
            var window = new MacWindow { Content = root, Width = 650, SizeToContent = SizeToContent.Height,
                ShowActivated = false, ShowInTaskbar = false, WindowStartupLocation = WindowStartupLocation.Manual,
                Left = SystemParameters.VirtualScreenLeft + SystemParameters.VirtualScreenWidth + 100, Top = 50 };
            window.Show(); await Task.Delay(100); window.UpdateLayout();
            static IEnumerable<GlyphRun> Glyphs(Drawing? drawing)
            {
                if (drawing is GlyphRunDrawing glyph) yield return glyph.GlyphRun;
                if (drawing is DrawingGroup group) foreach (var child in group.Children) foreach (var run in Glyphs(child)) yield return run;
            }
            var shortcutLabels = QualityChecks.Descendants(shortcutSamples).OfType<TextBlock>().ToArray();
            check(shortcutLabels.All(text => Glyphs(VisualTreeHelper.GetDrawing(text)).Any()) && shortcutLabels.SelectMany(text => Glyphs(VisualTreeHelper.GetDrawing(text))).All(run => run.GlyphIndices.All(index => index != 0)),
                mode + ": real modifier and punctuation keycaps render without missing glyphs");
            check(QualityChecks.Descendants(shortcutSamples).OfType<Border>().Where(cap => cap.Child is TextBlock).All(cap => cap.Child.RenderSize.Width <= cap.ActualWidth && cap.Child.RenderSize.Height <= cap.ActualHeight),
                mode + ": compact and recording keycaps contain their text without clipping");
            check(!shortcutLabels.Any(text => text.Text is "Quote" or "Slash" or "CommandOrControl"), mode + ": shortcut views do not expose internal key names");
            var primaryLabel = QualityChecks.Descendants(primary).OfType<TextBlock>().Single(text => text.Text == "确认连接");
            check(primaryLabel.FontSize == primary.FontSize, "button label inherits the control's font size instead of the generic text style");
            check(primaryLabel.Foreground is SolidColorBrush labelBrush && primary.Foreground is SolidColorBrush buttonBrush && labelBrush.Color == buttonBrush.Color,
                mode + ": primary action label inherits the contrasting foreground");
            ControlMotion.Busy(primary, true); window.UpdateLayout();
            check(primary.Content is string label && label == "确认连接", "busy feedback preserves the accessible action label");
            ControlMotion.Busy(primary, false); window.UpdateLayout();
            check((string)primary.Content == "确认连接", "busy feedback restores the regular glyph without replacing content");
            var tooltip = new ToolTip { Content = "复制邀请", PlacementTarget = secondary, Placement = PlacementMode.Bottom };
            tooltip.IsOpen = true; await Task.Delay(60); tooltip.UpdateLayout();
            check(tooltip.IsVisible && QualityChecks.Descendants(tooltip).OfType<TextBlock>().Any(text => text.Text == "复制邀请"), mode + ": nonempty hover help renders its actual text");
            tooltip.Content = ""; tooltip.UpdateLayout();
            check(tooltip.Visibility == Visibility.Collapsed && !tooltip.IsVisible, mode + ": empty-string hover help paints no background or padding");
            tooltip.Content = null; tooltip.UpdateLayout();
            check(tooltip.Visibility == Visibility.Collapsed && !tooltip.IsVisible, mode + ": null hover help paints no background or padding");
            tooltip.Content = "复制邀请"; tooltip.UpdateLayout();
            check(tooltip.IsVisible, mode + ": meaningful hover help becomes visible again after its content changes");
            tooltip.IsOpen = false;
            combo.IsDropDownOpen = true; await Task.Delay(280); combo.UpdateLayout();
            check(combo.Template.FindName("PART_Popup", combo) is Popup { IsOpen: true }, mode + ": styled selector opens its real popup");
            combo.SelectedIndex = 2; combo.IsDropDownOpen = false;
            check((string)combo.SelectedItem == "Iroh", mode + ": styled selector retains selection behavior");
            date.IsDropDownOpen = true; await Task.Delay(100);
            check(date.Template.FindName("PART_Popup", date) is Popup { IsOpen: true, Child: not null }, mode + ": date picker retains its calendar and popup parts");
            date.IsDropDownOpen = false;
            check(!QualityChecks.Descendants(date).OfType<Button>().SelectMany(button => QualityChecks.Descendants(button).OfType<TextBlock>()).Any(text => text.IsVisible), mode + ": calendar button contains only its icon, without the framework fallback label");
            QualityChecks.Capture(window, Path.Combine(directory, "controls-" + mode + ".png"));
            foreach (var page in new[] { "settings", "connections" })
            {
                var view = new MainWindow(page) { ShowActivated = false, ShowInTaskbar = false, WindowStartupLocation = WindowStartupLocation.Manual, Left = window.Left, Top = window.Top };
                view.Show(); await Task.Delay(250); view.UpdateLayout();
                QualityChecks.Capture(view, Path.Combine(directory, page + "-" + mode + ".png")); view.Close();
            }
            window.Close();
        }
        ThemeService.Apply("light", "canvas");
    }
}

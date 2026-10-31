using System.ComponentModel;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;

namespace TailSync.Wpf;

internal sealed class ShortcutRecorderWindow : MacWindow
{
    private readonly string _command;
    private readonly ContentControl _candidate;
    private readonly TextBlock _error;
    private readonly Button _save;
    private string _shortcut = "";
    private bool _busy;
    internal ShortcutRecorderWindow(Window owner, string field, string command)
    {
        _command = command; Owner = owner; Width = 440; MinWidth = 360; SizeToContent = SizeToContent.Height; ResizeMode = ResizeMode.NoResize; ShowInTaskbar = false;
        WindowStartupLocation = WindowStartupLocation.CenterOwner;
        Title = Localizer.Text(field == "sync_shortcut" ? "settings.shortcutDialogTitle" : "settings.historyShortcutDialogTitle");
        var root = new StackPanel(); root.Children.Add(MakeTitleBar(Title));
        var body = new StackPanel { Margin = new(24) };
        body.Children.Add(Ui.Text(Localizer.Text("settings.shortcutDialogPrompt"), 14));
        _candidate = new ContentControl { Content = Ui.Text(Localizer.Choose("请按下组合键", "Press a keyboard shortcut"), 22, "Accent"), HorizontalContentAlignment = HorizontalAlignment.Center };
        var keyWell = Ui.Surface(_candidate, new Thickness(16)); keyWell.Margin = new(0, 16, 0, 12); keyWell.SetResourceReference(Border.BackgroundProperty, "InputBrush"); body.Children.Add(keyWell);
        _error = Ui.Text("", 12, "Secondary"); body.Children.Add(_error);
        var cancel = Ui.Button(Localizer.Text("settings.cancel"), Close); cancel.IsCancel = true;
        _save = Ui.Button(Localizer.Choose("保存快捷键", "Save shortcut"), style: "PrimaryButton", icon: "check"); _save.IsEnabled = false; _save.IsDefault = true;
        _save.Click += async (_, _) => await Save();
        var actions = Ui.Stack(Orientation.Horizontal, 10, cancel, _save); actions.HorizontalAlignment = HorizontalAlignment.Right; actions.Margin = new(0, 20, 0, 0); body.Children.Add(actions);
        root.Children.Add(body); Content = root;
        PreviewKeyDown += Capture;
        Closing += (_, e) => { if (_busy) e.Cancel = true; };
    }
    internal async Task OpenAsync()
    {
        await NativeBackend.InvokeAsync("suspend_sync_shortcut");
        try { ShowDialog(); }
        finally { await NativeBackend.InvokeAsync("resume_sync_shortcut"); }
    }
    internal static string Display(string value) => ShortcutDisplayFormatter.Display(value);
    private void Capture(object sender, KeyEventArgs e)
    {
        if (_busy || e.IsRepeat || e.Key is Key.ImeProcessed or Key.DeadCharProcessed) return;
        var modifiers = Keyboard.Modifiers; var key = e.Key == Key.System ? e.SystemKey : e.Key;
        if (key == Key.Escape && modifiers == ModifierKeys.None) { e.Handled = true; Close(); return; }
        if (key == Key.Tab && modifiers is ModifierKeys.None or ModifierKeys.Shift) return;
        if (key is Key.LeftCtrl or Key.RightCtrl or Key.LeftShift or Key.RightShift or Key.LeftAlt or Key.RightAlt or Key.LWin or Key.RWin) return;
        // Once captured, Enter/Space operate the confirmation buttons normally.
        if (_shortcut.Length > 0 && modifiers == ModifierKeys.None && key is Key.Enter or Key.Space) return;
        e.Handled = true;
        if (modifiers == ModifierKeys.None)
        {
            _error.Text = Localizer.Text("settings.shortcutModifierRequired"); _save.IsEnabled = false; _shortcut = ""; return;
        }
        var token = KeyToken(key);
        if (token == null) { _error.Text = Localizer.Text("settings.shortcutUnsupported"); _save.IsEnabled = false; _shortcut = ""; return; }
        var keys = new List<string>();
        if (modifiers.HasFlag(ModifierKeys.Control)) keys.Add("CommandOrControl");
        if (modifiers.HasFlag(ModifierKeys.Alt)) keys.Add("Alt");
        if (modifiers.HasFlag(ModifierKeys.Shift)) keys.Add("Shift");
        if (modifiers.HasFlag(ModifierKeys.Windows)) keys.Add("Super");
        keys.Add(token); _shortcut = string.Join('+', keys); _candidate.Content = ShortcutDisplayFormatter.Keycaps(_shortcut, large: true);
        Ui.Name(_candidate, ShortcutDisplayFormatter.AccessibleName(_shortcut)); _error.Text = ""; _save.IsEnabled = true;
    }
    internal static string? KeyToken(Key key) => key switch
    {
        >= Key.A and <= Key.Z => key.ToString(),
        >= Key.D0 and <= Key.D9 => ((int)key - (int)Key.D0).ToString(),
        >= Key.F1 and <= Key.F24 => key.ToString(),
        >= Key.NumPad0 and <= Key.NumPad9 => "Numpad" + ((int)key - (int)Key.NumPad0),
        Key.Add => "NumpadAdd", Key.Subtract => "NumpadSubtract", Key.Multiply => "NumpadMultiply", Key.Divide => "NumpadDivide", Key.Decimal => "NumpadDecimal",
        Key.Pause => "Pause", Key.CapsLock => "CapsLock", Key.Scroll => "ScrollLock", Key.NumLock => "NumLock", Key.PrintScreen => "PrintScreen", Key.Tab => "Tab",
        Key.VolumeDown => "AudioVolumeDown", Key.VolumeUp => "AudioVolumeUp", Key.VolumeMute => "AudioVolumeMute", Key.MediaPlayPause => "MediaPlayPause", Key.MediaStop => "MediaStop", Key.MediaNextTrack => "MediaTrackNext", Key.MediaPreviousTrack => "MediaTrackPrevious",
        Key.Space => "Space", Key.Enter => "Enter", Key.Back => "Backspace", Key.Delete => "Delete", Key.Insert => "Insert",
        Key.Home => "Home", Key.End => "End", Key.PageUp => "PageUp", Key.PageDown => "PageDown", Key.Up => "Up", Key.Down => "Down", Key.Left => "Left", Key.Right => "Right", Key.Escape => "Escape",
        Key.OemComma => "Comma", Key.OemPeriod => "Period", Key.OemMinus => "Minus", Key.OemPlus => "Equal", Key.OemQuestion => "Slash", Key.OemSemicolon => "Semicolon", Key.OemQuotes => "Quote", Key.OemOpenBrackets => "BracketLeft", Key.OemCloseBrackets => "BracketRight", Key.OemPipe => "Backslash", Key.OemTilde => "Backquote",
        _ => null
    };
    private async Task Save()
    {
        if (_busy || _shortcut.Length == 0) return;
        _busy = true; _save.IsEnabled = false;
        try { await NativeBackend.InvokeAsync(_command, new { shortcut = _shortcut }); _busy = false; DialogResult = true; }
        catch (Exception error)
        {
            _error.Text = Localizer.Text("settings.shortcutConflict") + "\n" + error.Message;
            try { await NativeBackend.InvokeAsync("suspend_sync_shortcut"); } catch (Exception restore) { _error.Text += "\n" + restore.Message; }
        }
        finally { _busy = false; _save.IsEnabled = _shortcut.Length > 0; }
    }
}

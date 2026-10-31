using System.ComponentModel;
using System.Text.Json;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Shell;

namespace TailSync.Wpf;

internal sealed class PairingWindow : MacWindow
{
    private readonly TextBlock _peer = Ui.Text("", 14), _state = Ui.Text("", 12, "Accent"),
        _fingerprint = Ui.Text("", 11, "Secondary"), _remaining = Ui.Text("", 12, "Secondary"),
        _error = Ui.Text("", 12, "Warning"), _waitingCopy = Ui.Text("", 13);
    private readonly TextBlock[] _digits = new TextBlock[6];
    private readonly UniformGrid _code = new() { Columns = 6, HorizontalAlignment = HorizontalAlignment.Center };
    private readonly StackPanel _verification = new(), _waiting = new();
    private readonly Border _errorBox;
    private readonly ProgressBar _progress = new() { IsIndeterminate = true, Height = 3, Margin = new(0, 10, 0, 10) };
    private readonly Button _confirm, _cancel;
    private readonly CancellationTokenSource _lifetime = new();
    private bool _completed, _busy, _ownerWasEnabled;
    private int _operation;
    private string _directError = "";
    private JsonElement _status;
    private ModalShade? _shade;

    internal PairingWindow(Window owner, bool pollStatus = true)
    {
        Owner = owner; Width = 380; MinWidth = 340; SizeToContent = SizeToContent.Height;
        ResizeMode = ResizeMode.NoResize; ShowInTaskbar = false; WindowStartupLocation = WindowStartupLocation.CenterOwner;
        Title = Localizer.Text("settings.devicePairing");
        WindowChrome.SetWindowChrome(this, new WindowChrome { CaptionHeight = 0, ResizeBorderThickness = new(0), GlassFrameThickness = new(0) });
        KeyboardNavigation.SetTabNavigation(this, KeyboardNavigationMode.Cycle);
        var body = new StackPanel { Margin = new(24) };
        var shield = new VectorIcon { Kind = "shield", Width = 23, Height = 23 }; shield.SetResourceReference(VectorIcon.StrokeProperty, "AccentBrush");
        var icon = new Border { Child = shield, Width = 46, Height = 46, CornerRadius = new(14), BorderThickness = new(1), HorizontalAlignment = HorizontalAlignment.Center, Margin = new(0, 0, 0, 16) };
        icon.SetResourceReference(Border.BackgroundProperty, "AccentSoftBrush"); icon.SetResourceReference(Border.BorderBrushProperty, "BorderBrush"); body.Children.Add(icon);
        var heading = Ui.Text(Title); heading.SetResourceReference(TextBlock.FontFamilyProperty, "DisplayFont"); heading.SetResourceReference(TextBlock.FontSizeProperty, "SectionFontSize"); heading.FontWeight = FontWeights.SemiBold; heading.TextAlignment = TextAlignment.Center; body.Children.Add(heading);
        _peer.FontWeight = FontWeights.Medium; _peer.TextAlignment = TextAlignment.Center; _peer.Margin = new(0, 8, 0, 0); body.Children.Add(_peer);
        for (var i = 0; i < _digits.Length; i++)
        {
            var digit = Ui.Text("", 34); digit.FontWeight = FontWeights.SemiBold; digit.FontFamily = new("Cascadia Mono, Consolas"); digit.Width = 36; digit.Height = 52; digit.TextAlignment = TextAlignment.Center;
            _digits[i] = digit;
            var cell = new Border { Child = digit, CornerRadius = new(8), BorderThickness = new(1), Margin = new(3, 0, 3, 0), Padding = new(1, 0, 1, 0) };
            cell.SetResourceReference(Border.BackgroundProperty, "InputBrush"); cell.SetResourceReference(Border.BorderBrushProperty, "BorderBrush"); _code.Children.Add(cell);
        }
        _code.Margin = new(0, 18, 0, 9); _verification.Children.Add(_code);
        var compare = Ui.Text(Localizer.Text("settings.compareCode"), 12, "Secondary"); compare.TextAlignment = TextAlignment.Center; _verification.Children.Add(compare);
        _fingerprint.TextAlignment = TextAlignment.Center; _fingerprint.FontFamily = new("Cascadia Mono, Consolas"); _fingerprint.Margin = new(0, 9, 0, 0); _verification.Children.Add(_fingerprint); body.Children.Add(_verification);
        _waiting.Margin = new(0, 18, 0, 4); _waitingCopy.TextAlignment = TextAlignment.Center;
        _waiting.Children.Add(_progress); _waiting.Children.Add(_waitingCopy); body.Children.Add(_waiting);
        _state.TextAlignment = TextAlignment.Center; _state.Margin = new(0, 10, 0, 0); body.Children.Add(_state);
        _remaining.TextAlignment = TextAlignment.Center; _remaining.Margin = new(0, 8, 0, 0); body.Children.Add(_remaining);
        _errorBox = new Border { Child = _error, CornerRadius = new(8), Padding = new(10), Margin = new(0, 12, 0, 0), Visibility = Visibility.Collapsed };
        _errorBox.SetResourceReference(Border.BackgroundProperty, "WarningSoftBrush"); body.Children.Add(_errorBox);
        _cancel = Ui.Button(Localizer.Text("settings.cancel")); _cancel.Click += async (_, _) => await Cancel();
        _confirm = Ui.Button(Localizer.Text("settings.codesMatch"), style: "PrimaryButton", icon: "check"); _confirm.IsDefault = true; _confirm.Click += async (_, _) => await Confirm();
        _cancel.MinHeight = _confirm.MinHeight = 38; _cancel.HorizontalAlignment = _confirm.HorizontalAlignment = HorizontalAlignment.Stretch;
        var actions = new Grid { Margin = new(0, 20, 0, 0) }; actions.ColumnDefinitions.Add(new()); actions.ColumnDefinitions.Add(new());
        _cancel.Margin = new(0, 0, 5, 0); _confirm.Margin = new(5, 0, 0, 0); Grid.SetColumn(_confirm, 1); actions.Children.Add(_cancel); actions.Children.Add(_confirm); body.Children.Add(actions); Content = body;
        PreviewKeyDown += async (_, e) => { if (e.Key == Key.Escape && !_busy) { e.Handled = true; await Cancel(); } };
        Loaded += (_, _) => { _ownerWasEnabled = owner.IsEnabled; _shade = ModalShade.Show(owner); owner.IsEnabled = false; _cancel.Focus(); if (pollStatus) _ = Poll(); };
        Closing += ClosingPair;
        Closed += (_, _) => { _lifetime.Cancel(); _shade?.Dispose(); owner.IsEnabled = _ownerWasEnabled; if (owner.IsVisible && ShowActivated) owner.Activate(); };
    }
    internal void Apply(JsonElement status) { if (!_busy && !_completed) ApplySnapshot(status); }
    internal bool Completed => _completed;
    internal void ReportError(string message) { _directError = message; if (!_completed) ApplySnapshot(_status); }
    private void ApplySnapshot(JsonElement status)
    {
        _status = status;
        var peer = status.Object("peer"); var phase = status.String("phase");
        _peer.Text = peer.String("hostname", Localizer.Text("settings.waiting"));
        var code = new string(peer.String("verification_code").Where(char.IsAsciiDigit).ToArray());
        var verifying = code.Length == 6;
        for (var i = 0; i < _digits.Length; i++) _digits[i].Text = verifying ? code[i].ToString() : "";
        AutomationProperties.SetName(_code, Localizer.Text("settings.pairingCode") + " " + code);
        _verification.Visibility = verifying ? Visibility.Visible : Visibility.Collapsed;
        _waiting.Visibility = verifying ? Visibility.Collapsed : Visibility.Visible;
        _progress.Visibility = phase is "handshaking" or "finalizing" ? Visibility.Visible : Visibility.Collapsed;
        _waitingCopy.Text = Localizer.Text(phase == "handshaking" ? "settings.secureHandshake" : "settings.pairingInstruction");
        _fingerprint.Text = peer.String("fingerprint"); _fingerprint.ToolTip = _fingerprint.Text;
        _state.Text = Localizer.Text(phase == "finalizing" ? "settings.pairingFinalizing" : peer.Bool("local_confirmed") || phase == "waiting_for_peer" ? "settings.waitingPeerConfirm" : verifying ? "settings.compareCode" : "settings.pairingReady");
        _state.Visibility = verifying && phase == "verification" && !peer.Bool("local_confirmed") ? Visibility.Collapsed : Visibility.Visible;
        _remaining.Text = Localizer.Choose("剩余 ", "Expires in ") + Math.Max(0, status.Long("remaining_seconds")) + Localizer.Choose(" 秒", "s");
        _confirm.Content = Localizer.Text(peer.Bool("local_confirmed") ? "settings.confirmed" : "settings.codesMatch");
        _confirm.IsEnabled = !_busy && verifying && !peer.Bool("local_confirmed") && phase is "verification" or "waiting_for_peer";
        var statusError = status.String("error");
        var policyKey = status.String("failure_kind") switch
        {
            "protocol_incompatible" => "error.protocol_incompatible", "invalid_argument" => "error.invalid_argument",
            "identity_mismatch" => "error.pairing_identity_mismatch", "window_closed" => "error.pairing_window_closed",
            "timed_out" => "error.pairing_timed_out", "connection_closed" => "error.pairing_connection_closed",
            "busy" => "error.pairing_busy", "cancelled" => "error.pairing_cancelled",
            "transport" or "invalid_advertisement" => "error.internal", _ => "",
        };
        if (policyKey.Length > 0) statusError = Localizer.Text(policyKey);
        if (status.Object("error_info").ValueKind == JsonValueKind.Object) statusError = new BackendException(status.Object("error_info")).Message;
        _error.Text = _directError.Length > 0 ? _directError : statusError;
        if (phase is "timed_out" or "locked" or "cancelled")
        {
            _confirm.IsEnabled = false;
            if (_error.Text.Length == 0) _error.Text = Localizer.Choose("配对已结束，请关闭此窗口后重试。", "Pairing ended. Close this dialog and try again.");
        }
        _errorBox.Visibility = _error.Text.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
        if (phase == "paired") Complete();
    }
    private async Task Poll()
    {
        var token = _lifetime.Token;
        while (!token.IsCancellationRequested)
        {
            try
            {
                await Task.Delay(300, token); if (_busy) continue;
                var operation = _operation;
                var status = await NativeBackend.InvokeAsync<JsonElement>("get_pairing_status", token: token);
                if (operation == _operation && !_busy && !_completed) ApplySnapshot(status);
            }
            catch (OperationCanceledException) when (token.IsCancellationRequested) { return; }
            catch (Exception error) { if (!token.IsCancellationRequested) { _directError = error.Message; Apply(_status); } }
        }
    }
    private async Task Confirm()
    {
        if (_busy || !_confirm.IsEnabled) return;
        _busy = true; ++_operation; _directError = ""; _confirm.IsEnabled = _cancel.IsEnabled = false;
        try { ApplySnapshot(await NativeBackend.InvokeAsync<JsonElement>("confirm_pairing", token: _lifetime.Token)); }
        catch (Exception error) { _directError = error.Message; }
        finally { _busy = false; _cancel.IsEnabled = true; if (!_completed) ApplySnapshot(_status); }
    }
    private async Task Cancel()
    {
        if (_busy || _completed) return;
        _busy = true; ++_operation; _directError = ""; _cancel.IsEnabled = _confirm.IsEnabled = false;
        try { await NativeBackend.InvokeAsync("cancel_pairing", token: _lifetime.Token); Complete(); }
        catch (Exception error) { _directError = error.Message; }
        finally { _busy = false; _cancel.IsEnabled = true; if (!_completed) ApplySnapshot(_status); }
    }
    private async void ClosingPair(object? sender, CancelEventArgs e) { if (!_completed) { e.Cancel = true; await Cancel(); } }
    internal void Complete() { if (_completed) return; _completed = true; Close(); }
}

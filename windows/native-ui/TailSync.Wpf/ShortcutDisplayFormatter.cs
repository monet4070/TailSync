using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;

namespace TailSync.Wpf;

// Presentation only. The canonical tokens still go unchanged to the backend.
// CommandOrControl means Ctrl on Windows; Super means the Windows key.
internal static class ShortcutDisplayFormatter
{
    internal sealed record Keycap(string Label, string Name, bool Symbol = false, int ModifierOrder = 4);
    private static readonly FontFamily SymbolFont = new("Segoe UI Symbol");

    internal static Keycap[] Tokens(string shortcut) => shortcut.Split('+', StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries)
        .Select(Token).OrderBy(key => key.ModifierOrder).ToArray();

    internal static string Display(string shortcut) => string.IsNullOrWhiteSpace(shortcut)
        ? Localizer.Text("settings.shortcutDisabled") : string.Join(" + ", Tokens(shortcut).Select(key => key.Label));

    internal static string AccessibleName(string shortcut) => string.IsNullOrWhiteSpace(shortcut)
        ? Localizer.Text("settings.shortcutDisabled") : string.Join(" + ", Tokens(shortcut).Select(key => key.Name));

    private static Keycap Token(string token)
    {
        static Keycap Symbol(string label, string name) => new(label, name, true);
        switch (token.ToUpperInvariant())
        {
            case "COMMANDORCONTROL": case "CMDORCTRL": case "CONTROL": case "CTRL": return new("Ctrl", "Ctrl", ModifierOrder: 0);
            case "ALT": case "OPTION": return new("Alt", "Alt", ModifierOrder: 1);
            case "SHIFT": return new("Shift", "Shift", ModifierOrder: 2);
            case "SUPER": case "META": case "WIN": case "WINDOWS": case "COMMAND": case "CMD": return new("Win", "Win", ModifierOrder: 3);
            case "QUOTE": return new("'", Localizer.Choose("单引号", "Quote"));
            case "SLASH": return new("/", Localizer.Choose("斜杠", "Slash"));
            case "BACKSLASH": return new("\\", Localizer.Choose("反斜杠", "Backslash"));
            case "BACKQUOTE": return new("`", Localizer.Choose("反引号", "Backquote"));
            case "COMMA": return new(",", Localizer.Choose("逗号", "Comma"));
            case "PERIOD": return new(".", Localizer.Choose("句点", "Period"));
            case "MINUS": return new("-", Localizer.Choose("减号", "Minus"));
            case "EQUAL": return new("=", Localizer.Choose("等号", "Equal"));
            case "PLUS": return new("+", Localizer.Choose("加号", "Plus"));
            case "SEMICOLON": return new(";", Localizer.Choose("分号", "Semicolon"));
            case "BRACKETLEFT": return new("[", Localizer.Choose("左方括号", "Left bracket"));
            case "BRACKETRIGHT": return new("]", Localizer.Choose("右方括号", "Right bracket"));
            case "ENTER": case "RETURN": return new("Enter", "Enter");
            case "TAB": return new("Tab", "Tab");
            case "BACKSPACE": return new("Backspace", "Backspace");
            case "DELETE": case "DEL": return new("Del", "Delete");
            case "ESCAPE": case "ESC": return new("Esc", "Esc");
            case "CAPSLOCK": return new("Caps Lock", "Caps Lock");
            case "SPACE": return new("Space", Localizer.Choose("空格", "Space"));
            case "LEFT": case "ARROWLEFT": return Symbol("←", Localizer.Choose("左方向键", "Left arrow"));
            case "RIGHT": case "ARROWRIGHT": return Symbol("→", Localizer.Choose("右方向键", "Right arrow"));
            case "UP": case "ARROWUP": return Symbol("↑", Localizer.Choose("上方向键", "Up arrow"));
            case "DOWN": case "ARROWDOWN": return Symbol("↓", Localizer.Choose("下方向键", "Down arrow"));
            case "HOME": return new("Home", "Home");
            case "END": return new("End", "End");
            case "PAGEUP": return new("PgUp", "Page Up");
            case "PAGEDOWN": return new("PgDn", "Page Down");
            case "NUMPADADD": return new("Num +", Localizer.Choose("小键盘加号", "Numpad plus"));
            case "NUMPADSUBTRACT": return new("Num -", Localizer.Choose("小键盘减号", "Numpad minus"));
            case "NUMPADMULTIPLY": return new("Num ×", Localizer.Choose("小键盘乘号", "Numpad multiply"));
            case "NUMPADDIVIDE": return new("Num /", Localizer.Choose("小键盘除号", "Numpad divide"));
            case "NUMPADDECIMAL": return new("Num .", Localizer.Choose("小键盘小数点", "Numpad decimal"));
            case "NUMPADENTER": return new("Num Enter", Localizer.Choose("小键盘回车", "Numpad enter"));
            case "NUMPADEQUAL": return new("Num =", Localizer.Choose("小键盘等号", "Numpad equal"));
        }
        if (token.Length == 4 && token.StartsWith("Key", StringComparison.OrdinalIgnoreCase) && char.IsAsciiLetter(token[3]))
            return new(token[3..].ToUpperInvariant(), token[3..].ToUpperInvariant());
        if (token.Length == 6 && token.StartsWith("Digit", StringComparison.OrdinalIgnoreCase) && char.IsAsciiDigit(token[5]))
            return new(token[5..], token[5..]);
        if (token.Length == 7 && token.StartsWith("Numpad", StringComparison.OrdinalIgnoreCase) && char.IsAsciiDigit(token[6]))
            return new("Num " + token[6..], Localizer.Choose("小键盘 ", "Numpad ") + token[6..]);
        return new(token, token);
    }

    internal static StackPanel Keycaps(string shortcut, bool large = false)
    {
        var row = new StackPanel { Orientation = Orientation.Horizontal, VerticalAlignment = VerticalAlignment.Center, Name = "ShortcutKeycaps" };
        Ui.Name(row, AccessibleName(shortcut));
        if (string.IsNullOrWhiteSpace(shortcut)) { row.Children.Add(Ui.Text(Display(shortcut), 12, "Secondary")); return row; }
        foreach (var key in Tokens(shortcut))
        {
            var label = Ui.Text(key.Label, key.Symbol ? large ? 23 : 16 : large ? 20 : 13);
            label.TextAlignment = TextAlignment.Center; label.TextWrapping = TextWrapping.NoWrap;
            if (key.Symbol) label.FontFamily = SymbolFont;
            else label.FontWeight = FontWeights.Medium;
            var cap = Ui.Name(Ui.Surface(label, new Thickness(large ? 8 : 5, 0, large ? 8 : 5, 0)), key.Name);
            cap.CornerRadius = new(large ? 6 : 4); cap.MinWidth = cap.Height = large ? 34 : 24;
            cap.Margin = new(row.Children.Count == 0 ? 0 : 3, 0, 0, 0); row.Children.Add(cap);
        }
        return row;
    }
}

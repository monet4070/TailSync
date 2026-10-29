using Microsoft.UI.Xaml;

namespace TailSync.Native;

public partial class App : Application
{
    private Window? _window;
    public App() => InitializeComponent();
    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        var page = args.Arguments.Split(' ', StringSplitOptions.RemoveEmptyEntries).FirstOrDefault(x => x.StartsWith("--window="))?[9..] ?? "history";
        if (page is not ("history" or "favorites" or "settings" or "connections")) page = "history";
        _window = new MainWindow(page);
        _window.Activate();
    }
}

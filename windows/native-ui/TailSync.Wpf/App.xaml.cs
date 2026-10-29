using System.Windows;
using Microsoft.Win32;
namespace TailSync.Wpf;
public partial class App : Application
{
    protected override async void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);ThemeService.Apply();SystemEvents.UserPreferenceChanged+=PreferencesChanged;
        if(e.Args.Contains("--smoke-test")){ShutdownMode=ShutdownMode.OnExplicitShutdown;await SmokeChecks.Run(this,e.Args);return;}
        var page=e.Args.FirstOrDefault(x=>x.StartsWith("--window="))?[9..]??"history";
        MainWindow=TailSync.Wpf.MainWindow.Open(page is "history" or "favorites" or "settings" or "connections"?page:"history");
    }
    private void PreferencesChanged(object sender,UserPreferenceChangedEventArgs e)
    {
        if(ThemeService.Appearance=="system")Dispatcher.BeginInvoke(()=>ThemeService.Apply());
    }
    protected override void OnExit(ExitEventArgs e){SystemEvents.UserPreferenceChanged-=PreferencesChanged;base.OnExit(e);}
}

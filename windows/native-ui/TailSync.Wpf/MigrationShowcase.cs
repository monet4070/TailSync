using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Threading;

namespace TailSync.Wpf;

internal static class MigrationShowcase
{
    internal static async Task Run(App app,string directory)
    {
        directory=Path.GetFullPath(directory);Directory.CreateDirectory(directory);
        try
        {
            app.ShutdownMode=ShutdownMode.OnExplicitShutdown;
            await NativeBackend.StartAsync(isolatedRoot:Path.Combine(directory,"data-"+Guid.NewGuid().ToString("N")));
            AppSession.UseProduction(new NativeHistoryClient());
            await NativeBackend.InvokeAsync("seed_wpf_test_history",new{count=65});
            await SettingsState.ReloadAsync();await SettingsState.SaveAsync("language","zh-CN");await ThemeService.PersistAsync("light","builtin:canvas@1",false);
            NativeBackend.BeginWatching();
            var windows=new[]{MainWindow.Open("history"),MainWindow.Open("settings"),MainWindow.Open("connections")};
            var names=new[]{"history","settings","connections"};double left=24;
            foreach(var window in windows){window.Left=left;window.Top=64;left+=window.Width+24;}
            await Task.Delay(1200);await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
            for(var i=0;i<windows.Length;i++)QualityChecks.Capture(windows[i],Path.Combine(directory,names[i]+".png"));
            var settings=(ScrollViewer)windows[1].FindName("SettingsPage");settings.ScrollToEnd();await Task.Delay(250);
            QualityChecks.Capture(windows[1],Path.Combine(directory,"themes.png"));settings.ScrollToTop();
            var fonts=new List<object>();
            foreach(var theme in new[]{"canvas","flux","ledger","aura","mono"})
            {
                await ThemeService.PersistAsync("light","builtin:"+theme+"@1",false);await Task.Delay(100);
                QualityChecks.Capture(windows[0],Path.Combine(directory,"history-"+theme+".png"));
                fonts.Add(new{theme,historyFont=Application.Current.Resources["HistoryFont"].ToString(),searchFont=Application.Current.Resources["SearchFont"].ToString()});
            }
            await ThemeService.PersistAsync("light","builtin:canvas@1",false);
            await File.WriteAllTextAsync(Path.Combine(directory,"showcase.json"),JsonSerializer.Serialize(new{passed=true,pid=Environment.ProcessId,fonts,mode="Real WPF + real Rust service, isolated synthetic encrypted history; clipboard/network/registry integrations disabled",kind="Live WPF visual tree at monitor DPI"},new JsonSerializerOptions{WriteIndented=true}));
            app.MainWindow=windows[0];windows[0].Activate();
            var open=windows.Length;
            foreach(var window in windows)window.Closed+=async(_,_)=>{if(--open==0){await NativeBackend.StopAsync();app.Shutdown();}};
        }
        catch(Exception error){await File.WriteAllTextAsync(Path.Combine(directory,"error.txt"),error.ToString());await NativeBackend.StopAsync();app.Shutdown(1);}
    }
}

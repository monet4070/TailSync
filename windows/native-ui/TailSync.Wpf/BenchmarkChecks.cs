using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Threading;
namespace TailSync.Wpf;
internal static class BenchmarkChecks
{
    internal static async Task Run(App app,string directory)
    {
        try
        {
            Directory.CreateDirectory(directory);app.ShutdownMode=ShutdownMode.OnExplicitShutdown;
            var window=MainWindow.Open("history");await Task.Delay(200);
            await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
            if(window.ViewModel.Entries.Count!=30)throw new Exception("Expected 30 rendered entries");
            var list=(ListBox)window.FindName("HistoryList");var search=(TextBox)window.FindName("SearchBox");
            void Stage(string name)=>File.WriteAllText(Path.Combine(directory,"stage.txt"),name);
            Stage("idle");File.WriteAllText(Path.Combine(directory,"ready.json"),"{\"rows\":30}");
            while(!File.Exists(Path.Combine(directory,"go.txt")))await Task.Delay(50);
            Stage("search");var searches=new List<double>();var terms=new[]{"代码","article","absent-token",""};
            for(var i=0;i<80;i++)
            {
                var timer=Stopwatch.StartNew();search.Text=terms[i%4];
                while((window.ViewModel.Keyword!=terms[i%4] || list.Items.Count!=(i%4==2?0:30)) && timer.ElapsedMilliseconds<2000)await Task.Delay(1);
                await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);
                if(window.ViewModel.Keyword!=terms[i%4] || list.Items.Count!=(i%4==2?0:30))throw new Exception($"Search {i}: actual {list.Items.Count}, expected {(i%4==2?0:30)}, keyword {window.ViewModel.Keyword}, view-model count {window.ViewModel.Entries.Count}");
                searches.Add(timer.Elapsed.TotalMilliseconds);await Task.Delay(100);
            }
            Stage("scroll");var scrolls=new List<double>();
            for(var i=0;i<120;i++)
            {
                var timer=Stopwatch.StartNew();list.ScrollIntoView(list.Items[i%2==0?list.Items.Count-1:0]);
                await Dispatcher.Yield(DispatcherPriority.ApplicationIdle);scrolls.Add(timer.Elapsed.TotalMilliseconds);await Task.Delay(30);
            }
            Stage("settled");await Task.Delay(3000);
            Stage("multiwindow");var windows=new[]{MainWindow.Open("favorites"),MainWindow.Open("settings"),MainWindow.Open("connections")};
            await Task.Delay(4000);foreach(var extra in windows)extra.Close();
            Stage("closed");window.Close();await Task.Delay(6000);
            File.WriteAllText(Path.Combine(directory,"metrics.json"),JsonSerializer.Serialize(new{passed=true,search_ms=searches,scroll_dispatch_ms=scrolls,rendered_rows=30}));app.Shutdown();
        }
        catch(Exception e){File.WriteAllText(Path.Combine(directory,"error.txt"),e.ToString());app.Shutdown(1);}
    }
}

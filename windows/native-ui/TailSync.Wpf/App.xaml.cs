using System.Windows;
using System.Text.Json;
using System.Security.Principal;
using Microsoft.Win32;
namespace TailSync.Wpf;
public partial class App : Application
{
    private Mutex? _instance;
    private WindowsIntegration? _integration;
    private readonly List<string> _pendingLinks=[];
    private bool _productionReady,_activationPending;
    private bool _stopping;
    protected override async void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);ThemeService.Apply();SystemEvents.UserPreferenceChanged+=PreferencesChanged;
        const string onboardingCheckPrefix = "--clipboard-onboarding-check-dir=";
        var onboardingCheck = e.Args.FirstOrDefault(argument => argument.StartsWith(onboardingCheckPrefix));
        if (onboardingCheck != null) { await ClipboardOnboardingChecks.Run(this, onboardingCheck[onboardingCheckPrefix.Length..]); return; }
        const string resizePrefix = "--resize-check-dir=";
        var resizeCheck = e.Args.FirstOrDefault(argument => argument.StartsWith(resizePrefix));
        if (resizeCheck != null) { await ResizeChecks.Run(this, resizeCheck[resizePrefix.Length..], e.Args.Contains("--resize-review")); return; }
        const string activationFixturePrefix = "--activation-fixture-dir=";
        var activationFixture = e.Args.FirstOrDefault(argument => argument.StartsWith(activationFixturePrefix));
        if (activationFixture != null) { await ActivationChecks.Fixture(this, activationFixture[activationFixturePrefix.Length..]); return; }
        const string activationPrefix = "--activation-check-dir=";
        var activationCheck = e.Args.FirstOrDefault(argument => argument.StartsWith(activationPrefix));
        if (activationCheck != null) { await ActivationChecks.Run(this, activationCheck[activationPrefix.Length..]); return; }
        const string clarityPrefix = "--text-clarity-dir=";
        var claritySpecimen = e.Args.FirstOrDefault(argument => argument.StartsWith(clarityPrefix));
        if (claritySpecimen != null) { await TextClaritySpecimen.Run(this, claritySpecimen[clarityPrefix.Length..], e.Args.Contains("--clarity-review")); return; }
        const string serifPrefix = "--serif-specimen-dir=";
        var serifSpecimen = e.Args.FirstOrDefault(argument => argument.StartsWith(serifPrefix));
        if (serifSpecimen != null) { await SerifSpecimen.Run(this, serifSpecimen[serifPrefix.Length..]); return; }
        const string textRenderingPrefix = "--text-render-check-dir=";
        var textRenderingCheck = e.Args.FirstOrDefault(argument => argument.StartsWith(textRenderingPrefix));
        if (textRenderingCheck != null) { await TextRenderingChecks.Run(this, textRenderingCheck[textRenderingPrefix.Length..]); return; }
        const string thumbnailPrefix = "--thumbnail-check-dir=";
        var thumbnailCheck = e.Args.FirstOrDefault(argument => argument.StartsWith(thumbnailPrefix));
        if (thumbnailCheck != null) { await ThumbnailChecks.Run(this, thumbnailCheck[thumbnailPrefix.Length..], e.Args.Contains("--thumbnail-review")); return; }
        var connectionCheck = e.Args.FirstOrDefault(x => x.StartsWith("--connection-check-dir="));
        if (connectionCheck != null) { await ConnectionChecks.Run(this, connectionCheck[23..]); return; }
        var settingsReview = e.Args.FirstOrDefault(x => x.StartsWith("--settings-review-dir="));
        if (settingsReview != null) { await SettingsReview.Run(this, settingsReview[22..]); return; }
        var polish = e.Args.FirstOrDefault(x => x.StartsWith("--polish-check-dir="));
        if (polish != null) { await PolishChecks.Run(this, polish[19..]); return; }
        var quality=e.Args.FirstOrDefault(x=>x.StartsWith("--quality-dir="));
        if(quality!=null){ShutdownMode=ShutdownMode.OnExplicitShutdown;await QualityChecks.Run(this,quality[14..]);return;}
        const string showcasePrefix="--showcase-dir=";
        var showcase=e.Args.FirstOrDefault(x=>x.StartsWith(showcasePrefix));
        if(showcase!=null){await Showcase.Run(this,showcase[showcasePrefix.Length..]);return;}
        var benchmark=e.Args.FirstOrDefault(x=>x.StartsWith("--bench-dir="));
        if(benchmark!=null){await BenchmarkChecks.Run(this,benchmark[12..]);return;}
        if(e.Args.Contains("--smoke-test")){ShutdownMode=ShutdownMode.OnExplicitShutdown;await SmokeChecks.Run(this,e.Args);return;}
        var migration=e.Args.FirstOrDefault(x=>x.StartsWith("--migration-check-dir="));
        if(migration!=null){ShutdownMode=ShutdownMode.OnExplicitShutdown;await MigrationChecks.Run(this,migration[22..]);return;}
        var productionShowcase=e.Args.FirstOrDefault(x=>x.StartsWith("--migration-showcase-dir="));
        if(productionShowcase!=null){await MigrationShowcase.Run(this,productionShowcase[25..]);return;}
        var page=e.Args.FirstOrDefault(x=>x.StartsWith("--window="))?[9..]??"history";
        if(!e.Args.Contains("--demo"))
        {
            ShutdownMode=ShutdownMode.OnExplicitShutdown;
            var isolated=e.Args.FirstOrDefault(x=>x.StartsWith("--isolated-data="))?[16..];
            var link=e.Args.FirstOrDefault(WindowsIntegration.IsInvite);
            if(isolated==null&&!await AcquireInstance(link)){Shutdown();return;}
            try
            {
                await NativeBackend.StartAsync(isolatedRoot:isolated);
                AppSession.UseProduction(new TourHistoryClient(new NativeHistoryClient()));
                await SettingsState.ReloadAsync();
                await ThemeService.LoadAsync();
                NativeBackend.NavigationChanged+=Navigate;
                NativeBackend.ConnectionFailed+=BackendFailed;
                _productionReady=true;
                if(isolated==null)
                {
                    try{WindowsIntegration.RegisterUrlScheme();}
                    catch(Exception registration){System.Diagnostics.Debug.WriteLine(registration.Message);}
                    try{AutostartService.MigrateEnabledRegistration();}
                    catch(Exception registration){System.Diagnostics.Debug.WriteLine(registration.Message);}
                }
                NativeBackend.BeginWatching();
                if(link!=null)_pendingLinks.Add(link);
                if(_pendingLinks.Count>0)page="connections";
                foreach(var pending in _pendingLinks.ToArray())
                {
                    try{await NativeBackend.InvokeAsync("queue_remote_pairing_link",new{link=pending});}
                    catch(Exception invalidInvite){MessageBox.Show(invalidInvite.Message,Localizer.Choose("TailSync · 邀请不可用","TailSync · Invite unavailable"),MessageBoxButton.OK,MessageBoxImage.Warning);}
                }
                _pendingLinks.Clear();
                await GuidedTour.InitializeAsync(false);
            }
            catch(Exception error)
            {
                MessageBox.Show(error.Message,"TailSync · 启动失败",MessageBoxButton.OK,MessageBoxImage.Error);
                await NativeBackend.StopAsync();Shutdown(1);return;
            }
            if(link!=null)page="connections";
            if(e.Args.Contains("--background")&&!_activationPending&&link==null)return;
        }
        MainWindow=TailSync.Wpf.MainWindow.Open(page is "history" or "favorites" or "settings" or "connections"?page:"history");
        if(!AppSession.IsDemo)
        {
            try { await GuidedTour.InitializeAsync(!NativeBackend.IsIsolated && e.Args.Length==0); }
            catch(Exception error){System.Diagnostics.Debug.WriteLine(error.Message);}
        }
        foreach (var extra in e.Args.Where(argument => argument.StartsWith("--also-window=")).Select(argument => argument[14..]))
            if (extra is "settings" or "connections" or "history" or "favorites") TailSync.Wpf.MainWindow.Open(extra);
        var readyFile = e.Args.FirstOrDefault(argument => argument.StartsWith("--ready-file="))?[13..];
        if (readyFile != null)
            await System.IO.File.WriteAllTextAsync(readyFile, JsonSerializer.Serialize(new
            {
                pid = Environment.ProcessId, NativeBackend.IsConnected, NativeBackend.IsIsolated, NativeBackend.NetworkEnabled,
                connectionMode = SettingsState.Current.String("connection_mode"),
                windows = Windows.OfType<TailSync.Wpf.MainWindow>().Select(window => window.Page).ToArray(),
                display = Windows.OfType<TailSync.Wpf.MainWindow>().Select(window => new { window.Page, dpi = System.Windows.Media.VisualTreeHelper.GetDpi(window).DpiScaleX }).ToArray(),
                theme = ThemeService.ActiveThemeId,
            }, new JsonSerializerOptions { WriteIndented = true }));
    }
    private async Task<bool> AcquireInstance(string? link)
    {
        var user=WindowsIdentity.GetCurrent().User?.Value??Environment.UserName;
        var name=@"Local\TailSync.Wpf."+user;
        _instance=new Mutex(true,name,out var created);
        _integration=new WindowsIntegration(user);
        if(!created)
        {
            try{await _integration.ActivateAsync(link);}catch(Exception error){MessageBox.Show(error.Message,"TailSync",MessageBoxButton.OK,MessageBoxImage.Warning);}
            _instance.Dispose();_instance=null;return false;
        }
        _integration.Activated+=(_,pending)=>Dispatcher.BeginInvoke(async()=>
        {
            if(_stopping)return;
            if(!_productionReady){_activationPending=true;if(pending!=null)_pendingLinks.Add(pending);return;}
            try
            {
                if(pending!=null)await NativeBackend.InvokeAsync("queue_remote_pairing_link",new{link=pending});
                else MainWindow=TailSync.Wpf.MainWindow.Open("history");
            }
            catch(Exception error){MessageBox.Show(error.Message,"TailSync",MessageBoxButton.OK,MessageBoxImage.Warning);}
        });
        _integration.Listen();
        return true;
    }
    internal async void Navigate(object? sender,JsonElement navigation)
    {
        if(_stopping)return;
        try
        {
            var action=navigation.String("action");var page=navigation.String("window");
            if(action=="refresh")
            {
                if(page=="theme")await ThemeService.LoadAsync();
                else await SettingsState.ReloadAsync();
                return;
            }
            if(action=="quit"){await QuitAsync();return;}
            if(action=="tour"){await GuidedTour.StartAsync();return;}
            if(page is not ("history" or "favorites" or "settings" or "connections"))return;
            var existing=Windows.OfType<TailSync.Wpf.MainWindow>().FirstOrDefault(x=>x.Page==page);
            if(action=="close"||(action=="toggle"&&existing is{IsVisible:true,IsActive:true}&&existing.WindowState!=WindowState.Minimized))existing?.Close();
            else MainWindow=TailSync.Wpf.MainWindow.Open(page);
        }
        catch(Exception error){MessageBox.Show(error.Message,"TailSync",MessageBoxButton.OK,MessageBoxImage.Warning);}
    }
    private async void BackendFailed(object? sender,string error)
    {
        if(_stopping)return;
        _stopping=true;
        MessageBox.Show(Localizer.Choose("后台连接已中断：","Backend disconnected: ")+error,"TailSync",MessageBoxButton.OK,MessageBoxImage.Error);
        await NativeBackend.StopAsync();Shutdown(1);
    }
    private async Task QuitAsync()
    {
        if(_stopping)return;
        _stopping=true;await NativeBackend.StopAsync();Shutdown();
    }
    private void PreferencesChanged(object sender,UserPreferenceChangedEventArgs e)
    {
        if(ThemeService.Appearance=="system")Dispatcher.BeginInvoke(async()=>{try{if(NativeBackend.IsConnected)await ThemeService.LoadAsync();else ThemeService.Apply();}catch(Exception error){System.Diagnostics.Debug.WriteLine(error.Message);}});
    }
    protected override void OnExit(ExitEventArgs e)
    {
        _stopping=true;SystemEvents.UserPreferenceChanged-=PreferencesChanged;
        NativeBackend.NavigationChanged-=Navigate;NativeBackend.ConnectionFailed-=BackendFailed;
        _integration?.Dispose();
        if(_instance!=null){_instance.ReleaseMutex();_instance.Dispose();}
        Task.Run(NativeBackend.StopAsync).GetAwaiter().GetResult();
        base.OnExit(e);
    }
}

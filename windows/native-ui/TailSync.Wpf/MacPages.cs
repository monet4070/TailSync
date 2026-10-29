using System.Text.Json;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using TailSync.Native.Core;
namespace TailSync.Wpf;

public partial class MainWindow
{
    private ToggleButton? _syncSwitch;
    private static string T(string key)=>MacDesign.Text(key);
    private static Grid SettingRow(string title,string description,UIElement? control=null,double padding=13)
    {
        var row=new Grid { Margin=new(16,padding,16,padding), MinHeight=Math.Max(0,36-2*padding) };
        row.ColumnDefinitions.Add(new());row.ColumnDefinitions.Add(new(){Width=GridLength.Auto});
        var text=Ui.Text(title);var copy=new StackPanel { VerticalAlignment=VerticalAlignment.Center,Margin=new(0,0,8,0) };copy.Children.Add(text);
        if(description.Length>0){var desc=Ui.Text(description,10,"Tertiary");desc.Margin=new(0,2,0,0);copy.Children.Add(desc);}row.Children.Add(copy);
        if(control is FrameworkElement f){f.VerticalAlignment=VerticalAlignment.Center;Grid.SetColumn(f,1);row.Children.Add(f);}return row;
    }
    private static StackPanel Section(Panel parent,string title,string id)
    {
        var wrapper=new StackPanel { Margin=new(0,0,0,16),Tag=id };var heading=Ui.Text(title);heading.ClearValue(TextBlock.FontSizeProperty);heading.SetResourceReference(StyleProperty,"SectionTitle");wrapper.Children.Add(heading);
        var rows=new StackPanel();var card=Ui.Surface(rows);card.Margin=new(12,0,12,0);wrapper.Children.Add(card);parent.Children.Add(wrapper);return rows;
    }
    private static ToggleButton Switch(string title,bool value,Action<bool>? change=null)
    {
        var toggle=Ui.Name(new ToggleButton { IsChecked=value,VerticalAlignment=VerticalAlignment.Center },title);toggle.SetResourceReference(StyleProperty,"MacSwitch");
        if(change!=null)toggle.Click+=(_,_)=>change(toggle.IsChecked==true);return toggle;
    }
    private static Border Segments(string name,(string Id,string Label)[] options,string value,double width,Action<string> changed)
    {
        var panel=new UniformGrid { Columns=options.Length };var group=Guid.NewGuid().ToString("N");
        foreach(var (id,label) in options){var b=Ui.Name(new RadioButton { GroupName=group,Content=label,IsChecked=id==value,Tag=id },label);b.SetResourceReference(StyleProperty,"SegmentButton");b.Checked+=(_,_)=>changed(id);panel.Children.Add(b);}
        var border=new Border { Child=panel,Padding=new(2),CornerRadius=new(6),Width=width,Height=25 };border.SetResourceReference(Border.BackgroundProperty,"InputBrush");AutomationProperties.SetName(border,name);return border;
    }
    private void BuildSettings()
    {
        var guide=Ui.Button("新手引导",()=>DemoDialog.Inform(this,T("onboarding.title"),T("onboarding.reopen")+"\n\n此版本使用示例数据，双击记录演示恢复，空格打开预览。"));
        guide.HorizontalContentAlignment=HorizontalAlignment.Stretch;guide.Padding=new(16);guide.Margin=new(16,0,16,16);guide.Height=66;
        var copy=new Grid();var title=Ui.Text(T("onboarding.title"));title.FontWeight=FontWeights.Bold;var desc=Ui.Text(T("onboarding.reopen"),10,"Secondary");desc.Margin=new(0,4,0,0);copy.Children.Add(Ui.Stack(Orientation.Vertical,0,title,desc));var arrow=new VectorIcon { Kind="arrow",HorizontalAlignment=HorizontalAlignment.Right,Width=18,Height=18 };copy.Children.Add(arrow);guide.Content=copy;SettingsContent.Children.Add(guide);
        foreach(var section in MacDesign.Root.GetProperty("sections").EnumerateArray())
        {
            var id=section.GetProperty("id").GetString()!;var rows=Section(SettingsContent,T("settings."+id),id);
            foreach(var row in section.GetProperty("rows").EnumerateArray()){if(rows.Children.Count>0)Ui.Divider(rows);rows.Children.Add(BuildSetting(row));}
        }
    }
    private UIElement BuildSetting(JsonElement spec)
    {
        var id=spec.GetProperty("id").GetString()!;var type=spec.GetProperty("type").GetString();
        var title=T(spec.GetProperty("title").GetString()!);var desc=T(spec.GetProperty("description").GetString()!);UIElement? control=null;
        switch(type)
        {
            case "switch":
                var fallback=spec.TryGetProperty("value",out var v)&&v.GetBoolean();
                control=Switch(title,id=="syncEnabled"?AppSession.Sync:AppSession.Flag(id,fallback),value=>{if(id=="syncEnabled")AppSession.SetSync(value);else AppSession.SetFlag(id,value);});
                if(id=="syncEnabled")_syncSwitch=(ToggleButton)control;break;
            case "shortcut":control=Shortcut(spec);break;
            case "update":
                var status=Ui.Text(T("settings.updateReady"),10,"Secondary");var version=Ui.Stack(Orientation.Vertical,2,Ui.Text(T("settings.currentVersion")+": 0.1.0"),status);
                var update=Ui.Button(T("settings.updateCheck"),()=>status.Text="演示："+T("settings.updateCurrent"));
                var updateRow=SettingRow("","",update);((StackPanel)updateRow.Children[0]).Children.Clear();((StackPanel)updateRow.Children[0]).Children.Add(Ui.Stack(Orientation.Horizontal,8,new VectorIcon { Kind="download",Width=24,Height=24 },version));return updateRow;
            case "slider":
                var slider=Ui.Name(new Slider { Minimum=spec.GetProperty("min").GetInt32(),Maximum=spec.GetProperty("max").GetInt32(),TickFrequency=spec.GetProperty("step").GetInt32(),SmallChange=10,LargeChange=10,Value=AppSession.HistoryLimit,Width=180 },title);
                var valueLabel=Ui.Text(AppSession.HistoryLimit.ToString(),12,"Accent");valueLabel.HorizontalAlignment=HorizontalAlignment.Center;
                var badge=new Border { Child=valueLabel,Width=46,Height=26,CornerRadius=new(9) };badge.SetResourceReference(Border.BackgroundProperty,"AccentSoftBrush");
                var row=SettingRow(title,desc.Replace("{value}",AppSession.HistoryLimit.ToString()),Ui.Stack(Orientation.Horizontal,8,slider,badge));
                slider.ValueChanged+=(_,e)=>{AppSession.HistoryLimit=(int)e.NewValue;valueLabel.Text=((int)e.NewValue).ToString();var labels=(StackPanel)row.Children[0];((TextBlock)labels.Children[1]).Text=desc.Replace("{value}",((int)e.NewValue).ToString());};return row;
            case "disclosure":
                var body=new StackPanel { Margin=new(16,0,16,10),Visibility=Visibility.Collapsed };foreach(var child in spec.GetProperty("rows").EnumerateArray()){if(body.Children.Count>0)Ui.Divider(body);body.Children.Add(BuildSetting(child));}
                return Disclosure(title,desc,body);
            case "automatic":control=Ui.Text(T("settings.imageAccessAutomatic"),11,"Secondary");break;
            case "storage":
                // Display-only path from the reference layout; never inspect the real folder.
                var storage=SettingRow("~/Library/Application Support/TailSync","125 MB / 10 GB",Ui.Button(T("settings.storageChange"),()=>Notify("存储界面演示；未迁移真实文件")));
                return storage;
            case "stepper":
                var quota=AppSession.QuotaGiB;var quotaLabel=Ui.Text(quota+" GiB",12);var inc=Ui.Button("⌃",()=>{quota=Math.Min(16384,quota+1);AppSession.QuotaGiB=quota;quotaLabel.Text=quota+" GiB";});var dec=Ui.Button("⌄",()=>{quota=Math.Max(1,quota-1);AppSession.QuotaGiB=quota;quotaLabel.Text=quota+" GiB";});
                foreach(var b in new[]{inc,dec}){b.Width=20;b.Height=12;b.MinHeight=12;b.Padding=new(0);}
                control=Ui.Stack(Orientation.Horizontal,8,quotaLabel,Ui.Stack(Orientation.Vertical,0,inc,dec));break;
            case "connections":control=Ui.Button(T("settings.openConnections"),()=>Open("connections"));break;
            case "appearance":control=Segments(title,[("system",T("settings.themeSystem")),("light",T("settings.themeLight")),("dark",T("settings.themeDark"))],ThemeService.Appearance,190,value=>ThemeService.Apply(appearance:value));break;
            case "themes":return ThemeCards(title,desc);
            case "language":
                var language=Ui.Name(new ComboBox { Width=130,FontSize=11 },title);language.Items.Add("English");language.Items.Add("简体中文");language.SelectedIndex=1;language.SelectionChanged+=(_,_)=>Notify("语言选择界面演示；当前原型文案使用简体中文");control=language;break;
        }
        var result=SettingRow(title,desc,control);result.Tag=id;return result;
    }
    private static StackPanel Disclosure(string title,string description,FrameworkElement body,VectorIcon? leading=null)
    {
        var indicator=new VectorIcon { Kind="chevron",Width=10,Height=10,LayoutTransform=new RotateTransform(0) };
        var row=SettingRow(title,description,indicator,10);
        if(leading!=null){var text=(StackPanel)row.Children[0];row.Children.Remove(text);row.Children.Insert(0,Ui.Stack(Orientation.Horizontal,8,leading,text));}
        var toggle=Ui.Button(title,()=>{var open=body.Visibility!=Visibility.Visible;body.Visibility=open?Visibility.Visible:Visibility.Collapsed;indicator.LayoutTransform=new RotateTransform(open?90:0);},"PlainButton");
        toggle.HorizontalContentAlignment=HorizontalAlignment.Stretch;toggle.Content=row;return Ui.Stack(Orientation.Vertical,0,toggle,body);
    }
    private FrameworkElement Shortcut(JsonElement spec)
    {
        var original=spec.GetProperty("keys").EnumerateArray().Select(x=>x.GetString()!).ToArray();var shortcutId=spec.GetProperty("id").GetString()!;var saved=AppSession.Shortcuts.GetValueOrDefault(shortcutId,original);var recording=false;
        var caps=new StackPanel { Orientation=Orientation.Horizontal };
        void Render(string[] keys)
        {
            caps.Children.Clear();caps.Children.Add(new VectorIcon { Kind="keyboard",Width=11,Height=11,Margin=new(0,0,7,0) });
            foreach(var key in keys){var label=Ui.Text(key,11);label.FontWeight=FontWeights.SemiBold;var badge=Ui.Surface(label,new(6,3,6,3));badge.CornerRadius=new(4);badge.Margin=new(3,0,0,0);caps.Children.Add(badge);}
            caps.Children.Add(new VectorIcon { Kind="edit",Width=11,Height=11,Margin=new(7,0,0,0) });
        }
        Render(saved);var recorder=Ui.Button(T(spec.GetProperty("title").GetString()!));recorder.Content=caps;recorder.Padding=new(8,4,8,4);
        recorder.Click+=(_,_)=>{recording=true;Render([T("settings.shortcutRecording")]);recorder.Focus();};
        recorder.PreviewKeyDown+=(_,e)=>{
            if(!recording)return;var key=e.Key==Key.System?e.SystemKey:e.Key;if(key is Key.LeftAlt or Key.RightAlt or Key.LeftCtrl or Key.RightCtrl or Key.LeftShift or Key.RightShift or Key.LWin or Key.RWin)return;
            e.Handled=true;recording=false;if(key==Key.Escape){Render(saved);return;}
            var tokens=new List<string>();foreach(var (flag,token) in new[]{(ModifierKeys.Control,"⌃"),(ModifierKeys.Alt,"⌥"),(ModifierKeys.Shift,"⇧"),(ModifierKeys.Windows,"⌘")})if(Keyboard.Modifiers.HasFlag(flag))tokens.Add(token);
            tokens.Add(key.ToString());saved=tokens.ToArray();AppSession.Shortcuts[shortcutId]=saved;Render(saved);
        };
        recorder.LostKeyboardFocus+=(_,_)=>{if(recording){recording=false;Render(saved);}};
        var reset=Ui.IconButton("refresh","重置快捷键",()=>{saved=["⇧","⌘",spec.GetProperty("id").GetString()=="syncShortcut"?"S":"H"];AppSession.Shortcuts[shortcutId]=saved;Render(saved);});
        var clear=Ui.IconButton("close",T("settings.clearShortcut"),()=>{saved=[];AppSession.Shortcuts[shortcutId]=saved;Render([T("settings.shortcutNone")]);});
        foreach(var b in new[]{reset,clear}){b.Width=b.Height=24;b.BorderThickness=new(1);b.SetResourceReference(StyleProperty,typeof(Button));b.Padding=new(4);}
        return Ui.Stack(Orientation.Horizontal,6,recorder,reset,clear);
    }
    private FrameworkElement ThemeCards(string title,string description)
    {
        var wrapper=Ui.Stack(Orientation.Vertical,0,Ui.Text(title),Ui.Text(description,10,"Tertiary"));wrapper.Margin=new(16,12,16,12);
        var grid=new UniformGrid { Columns=2,Margin=new(-4,10,-4,10) };
        var buttons=new List<(Button Button,string Id,Border Check)>();
        foreach(var theme in MacDesign.Root.GetProperty("themes").EnumerateArray())
        {
            var id=theme.GetProperty("id").GetString()!;var accent=(Brush)new BrushConverter().ConvertFromString(theme.GetProperty("accent").GetString()!)!;
            var band=new Grid { Height=68,Background=(Brush)new BrushConverter().ConvertFromString(theme.GetProperty("canvas").GetString()!)! };
            band.Children.Add(new Border { Width=24,Height=24,CornerRadius=new(5),Background=accent,HorizontalAlignment=HorizontalAlignment.Left,VerticalAlignment=VerticalAlignment.Top,Margin=new(10) });
            var lines=new StackPanel { Margin=new(42,14,10,0),VerticalAlignment=VerticalAlignment.Top };for(var i=0;i<3;i++)lines.Children.Add(new Border { Height=5,Margin=new(0,0,i==2?35:0,5),CornerRadius=new(2),Background=accent,Opacity=.2 });band.Children.Add(lines);
            var check=new Border { Child=new VectorIcon { Kind="check",Width=12,Height=12,Stroke=Brushes.White },Width=20,Height=20,CornerRadius=new(10),Background=accent,HorizontalAlignment=HorizontalAlignment.Right,VerticalAlignment=VerticalAlignment.Bottom,Margin=new(8),Visibility=id==ThemeService.ThemeId?Visibility.Visible:Visibility.Collapsed };band.Children.Add(check);
            var label=new DockPanel { Margin=new(9,7,9,7),LastChildFill=false };var name=Ui.Text(theme.GetProperty("name").GetString()!,12);DockPanel.SetDock(name,Dock.Left);label.Children.Add(name);var built=Ui.Text(T("settings.themePackageBuiltIn"),10,"Tertiary");DockPanel.SetDock(built,Dock.Right);label.Children.Add(built);
            var button=Ui.Button(theme.GetProperty("name").GetString()!,()=>{
                ThemeService.Apply(id:id);foreach(var (other,otherId,stamp) in buttons){stamp.Visibility=otherId==id?Visibility.Visible:Visibility.Collapsed;other.SetResourceReference(Control.BorderBrushProperty,otherId==id?"AccentBrush":"BorderBrush");}
            });button.Content=Ui.Stack(Orientation.Vertical,0,band,label);button.Padding=new(0);button.Margin=new(4);button.HorizontalContentAlignment=HorizontalAlignment.Stretch;button.MinHeight=108;
            button.SetResourceReference(Control.BorderBrushProperty,id==ThemeService.ThemeId?"AccentBrush":"BorderBrush");buttons.Add((button,id,check));grid.Children.Add(button);
        }
        grid.SizeChanged+=(_,e)=>grid.Columns=Math.Max(1,(int)(e.NewSize.Width/198));wrapper.Children.Add(grid);
        wrapper.Children.Add(Ui.Button(T("settings.themePackageImport"),()=>Notify("主题包导入界面演示；未安装文件")));return wrapper;
    }
    private void BuildConnections()
    {
        var rows=Section(ConnectionsContent,T("connections.title"),"connections");var remote=new StackPanel { Visibility=AppSession.ConnectionMode is "auto" or "iroh"?Visibility.Visible:Visibility.Collapsed };
        rows.Children.Add(SettingRow(T("settings.connectionMode"),T("settings.connectionModeDescription"),Segments("连接方式",[("auto",T("settings.modeAuto")),("lan",T("settings.modeLan")),("iroh",T("settings.modeIroh")),("tailscale",T("settings.modeTailscale"))],AppSession.ConnectionMode,280,mode=>{AppSession.ConnectionMode=mode;remote.Visibility=mode is "auto" or "iroh"?Visibility.Visible:Visibility.Collapsed;})));Ui.Divider(rows);
        var identity=SettingRow("MacBook Pro  ·  "+T("settings.localDevice"),"D4:19:8E:2F:70:3A:6B:91\niroh: 9b41c6f8…e731a205");rows.Children.Add(identity);Ui.Divider(rows);
        rows.Children.Add(SettingRow("2 "+T("settings.devices"),"",Ui.IconButton("refresh",T("settings.refresh"),()=>Notify("已刷新示例设备")),6));
        foreach(var (name,status,on) in new[]{("DESKTOP-V62KRAF","192.168.1.106 · en0 · 局域网 · 7 ms",true),("Studio PC","离线 · Tailscale",false)})
        {
            var actions=Ui.Stack(Orientation.Horizontal,6,Ui.IconButton("latency",T("settings.testAllConnections"),()=>Notify("演示延迟：7 ms")),Ui.Button(T("settings.unpair"),()=>Notify("未移除真实设备")),Switch(name+" 同步",on));
            rows.Children.Add(SettingRow("● "+name+"  ✓",status+"\n8F:62:41:BE:10:D2:59:AC",actions));
        }
        Ui.Divider(rows);rows.Children.Add(SettingRow(T("settings.pairDevice"),T("settings.pairingClosed"),Ui.Button(T("settings.allowPairing"),()=>DemoDialog.Pair(this))));Ui.Divider(rows);
        var body=new StackPanel { Margin=new(16,0,16,16),Visibility=Visibility.Collapsed };
        body.Children.Add(SettingRow(T("settings.createRemoteInvite"),T("settings.createRemoteInviteDescription"),Ui.Button(T("settings.createRemoteInvite"),()=>Notify("邀请界面演示；未发送网络请求")),0));
        var use=Ui.Text(T("settings.useRemoteInvite"),12);use.Margin=new(0,14,0,2);body.Children.Add(use);body.Children.Add(Ui.Text(T("settings.useRemoteInviteDescription"),10,"Secondary"));
        body.Children.Add(Ui.Name(new TextBox { Tag=T("settings.remoteInvitePlaceholder"),FontSize=11,Margin=new(0,6,0,8) },T("settings.remoteInviteLink")));
        var actionsBar=Ui.Stack(Orientation.Horizontal,8,Ui.Button(T("settings.checkInvite"),()=>Notify("邀请检查演示；未连接真实设备")),Ui.Button(T("settings.startRemotePairing"),()=>DemoDialog.Pair(this)));actionsBar.HorizontalAlignment=HorizontalAlignment.Right;body.Children.Add(actionsBar);
        remote.Children.Add(Disclosure(T("settings.remotePairing"),T("settings.remotePairingDescription"),body,new VectorIcon { Kind="globe",Width=24,Height=24 }));rows.Children.Add(remote);
    }
}

using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Shell;
using TailSync.Native.Core;
namespace TailSync.Wpf;

internal sealed class DemoDialog : MacWindow
{
    private DemoDialog(Window owner,string title,UIElement body,string action,bool cancel)
    {
        Owner=owner;Title=title;Width=400;MinWidth=360;SizeToContent=SizeToContent.Height;WindowStartupLocation=WindowStartupLocation.CenterOwner;ResizeMode=ResizeMode.NoResize;ShowInTaskbar=false;
        WindowChrome.SetWindowChrome(this,new WindowChrome { CaptionHeight=0,ResizeBorderThickness=new(0),GlassFrameThickness=new(0),CornerRadius=new(12) });
        var panel=new StackPanel { Margin=new(24) };panel.Children.Add(body);
        var buttons=new Grid { Margin=new(0,24,0,0) };
        buttons.ColumnDefinitions.Add(new()); if(cancel) buttons.ColumnDefinitions.Add(new());
        Button? initialFocus=null;
        if(cancel){var back=Ui.Button(AppSession.IsDemo ? MacDesign.Text("settings.cancel") : Localizer.Text("settings.cancel"),()=>DialogResult=false);back.IsCancel=true;back.MinHeight=38;back.Margin=new(0,0,5,0);buttons.Children.Add(back);initialFocus=back;}
        var confirm=Ui.Button(action,()=>DialogResult=true,"PrimaryButton");confirm.MinHeight=38;confirm.IsDefault=true;
        if(cancel){confirm.Margin=new(5,0,0,0);Grid.SetColumn(confirm,1);}buttons.Children.Add(confirm);panel.Children.Add(buttons);Content=panel;
        KeyboardNavigation.SetTabNavigation(this,KeyboardNavigationMode.Cycle);
        ModalShade? shade=null;Loaded+=(_,_)=>{shade=ModalShade.Show(owner);var focus=initialFocus??confirm;FocusManager.SetFocusedElement(this,focus);focus.Focus();};Closed+=(_,_)=>shade?.Dispose();
        PreviewKeyDown+=(_,e)=>{if(e.Key==Key.Escape){e.Handled=true;DialogResult=false;}};
    }
    public static bool Confirm(Window owner,string title,string message,string action)
    {
        var heading=Ui.Text(title);heading.SetResourceReference(TextBlock.FontSizeProperty,"SectionFontSize");heading.SetResourceReference(TextBlock.FontFamilyProperty,"DisplayFont"); heading.FontWeight = FontWeights.SemiBold;var body=Ui.Stack(Orientation.Vertical,14,heading,Ui.Text(message,14,"Secondary"));
        return new DemoDialog(owner,title,body,action,true).ShowDialog()==true;
    }
    public static void Inform(Window owner,string title,string message)
    {
        var heading=Ui.Text(title);heading.SetResourceReference(TextBlock.FontSizeProperty,"SectionFontSize");heading.SetResourceReference(TextBlock.FontFamilyProperty,"DisplayFont"); heading.FontWeight = FontWeights.SemiBold;new DemoDialog(owner,title,Ui.Stack(Orientation.Vertical,14,heading,Ui.Text(message,14,"Secondary")),Localizer.Choose("知道了","Got it"),false).ShowDialog();
    }
    public static void Pair(Window owner)
    {
        var shield=new VectorIcon { Kind="shield",Width=28,Height=28,HorizontalAlignment=HorizontalAlignment.Center };shield.SetResourceReference(VectorIcon.StrokeProperty,"AccentBrush");
        var name=Ui.Text("DESKTOP-V62KRAF",17);name.HorizontalAlignment=HorizontalAlignment.Center;name.SetResourceReference(TextBlock.FontFamilyProperty,"DisplayFont");
        var code=Ui.Text("829 416",34);code.FontFamily=new("Cascadia Mono, Consolas");code.FontWeight=FontWeights.Bold;code.HorizontalAlignment=HorizontalAlignment.Center;
        var explanation=Ui.Text(MacDesign.Text("settings.compareCode"),12,"Secondary");explanation.TextAlignment=TextAlignment.Center;
        var fingerprint=Ui.Text("8F:62:41:BE:10:D2:59:AC",10,"Tertiary");fingerprint.FontFamily=new("Cascadia Mono, Consolas");fingerprint.TextAlignment=TextAlignment.Center;
        var body=Ui.Stack(Orientation.Vertical,14,shield,name,code,explanation,fingerprint);body.MinHeight=220;
        new DemoDialog(owner,MacDesign.Text("settings.pairDevice"),body,MacDesign.Text("settings.codesMatch"),true).ShowDialog();
    }
}

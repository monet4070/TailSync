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
        Owner=owner;Title=title;Width=340;SizeToContent=SizeToContent.Height;WindowStartupLocation=WindowStartupLocation.CenterOwner;ResizeMode=ResizeMode.NoResize;ShowInTaskbar=false;
        WindowChrome.SetWindowChrome(this,new WindowChrome { CaptionHeight=0,ResizeBorderThickness=new(0),GlassFrameThickness=new(0),CornerRadius=new(12) });
        var panel=new StackPanel { Margin=new(24) };panel.Children.Add(body);
        var buttons=new StackPanel { Orientation=Orientation.Horizontal,HorizontalAlignment=HorizontalAlignment.Right,Margin=new(0,20,0,0) };
        if(cancel){var back=Ui.Button(MacDesign.Text("settings.cancel"),()=>DialogResult=false);back.IsCancel=true;buttons.Children.Add(back);}
        var confirm=Ui.Button(action,()=>DialogResult=true,"PrimaryButton");confirm.Margin=new(cancel?10:0,0,0,0);confirm.IsDefault=true;buttons.Children.Add(confirm);panel.Children.Add(buttons);Content=panel;
        PreviewKeyDown+=(_,e)=>{if(e.Key==Key.Escape){e.Handled=true;DialogResult=false;}};
    }
    public static bool Confirm(Window owner,string title,string message,string action)
    {
        var heading=Ui.Text(title,17);heading.SetResourceReference(TextBlock.FontFamilyProperty,"DisplayFont");var body=Ui.Stack(Orientation.Vertical,14,heading,Ui.Text(message,13));
        return new DemoDialog(owner,title,body,action,true).ShowDialog()==true;
    }
    public static void Inform(Window owner,string title,string message)
    {
        var heading=Ui.Text(title,17);heading.SetResourceReference(TextBlock.FontFamilyProperty,"DisplayFont");new DemoDialog(owner,title,Ui.Stack(Orientation.Vertical,14,heading,Ui.Text(message,13)),"知道了",false).ShowDialog();
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

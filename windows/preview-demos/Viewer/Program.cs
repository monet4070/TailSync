using System.Windows;

namespace TailSync.PreviewDemo;

public static class Program
{
    [STAThread]
    public static int Main(string[] args)
    {
        try
        {
            var options = Options.Parse(args);
            var app = new Application { ShutdownMode = ShutdownMode.OnExplicitShutdown };
            var window = new PreviewWindow(options);
            return app.Run(window);
        }
        catch (Exception error)
        {
            // Benchmark launchers must receive a nonzero exit, never hang on a modal error dialog.
            if (!args.Contains("--benchmark")) MessageBox.Show(error.ToString(), "TailSync Preview Demo");
            Console.Error.WriteLine(error);
            return 2;
        }
    }
}

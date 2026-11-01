using Microsoft.UI.Xaml;
namespace RustClipSync;
public sealed partial class MainWindow : Window
{
    public MainWindow()
    {
        InitializeComponent();
        WindowHost.Children.Add(new MainView(this));
    }
}

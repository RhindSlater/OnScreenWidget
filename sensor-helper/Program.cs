using System.Globalization;
using System.Text;
using LibreHardwareMonitor.Hardware;

// Streams every sensor as one JSON line per interval on stdout:
// {"t":[{"l":"CPU / Temperatures / Core #1","v":47.0,"u":"°C"}, ...]}
// Needs elevation for CPU core / Super I/O / SMBus (RAM) access.
// Exits when the parent closes stdin (app quit/crash) so the driver is released.
Console.OutputEncoding = new UTF8Encoding(false);
int intervalMs = 1000;
if (args.Length > 0 && int.TryParse(args[0], out var a) && a >= 250) intervalMs = a;

var pc = new Computer
{
    IsCpuEnabled = true,
    IsGpuEnabled = true,
    IsMemoryEnabled = true,
    IsMotherboardEnabled = true,
    IsStorageEnabled = true,
};
pc.Open();

var quit = new ManualResetEventSlim(false);
new Thread(() =>
{
    try { while (Console.In.ReadLine() != null) { } } catch { }
    quit.Set();
}) { IsBackground = true }.Start();

static string Unit(SensorType t) => t switch
{
    SensorType.Temperature => "°C",
    SensorType.Fan => "RPM",
    SensorType.Voltage => "V",
    SensorType.Clock => "MHz",
    SensorType.Power => "W",
    SensorType.Load or SensorType.Control or SensorType.Level or SensorType.Humidity => "%",
    SensorType.Current => "A",
    SensorType.Data => "GB",
    SensorType.SmallData => "MB",
    SensorType.Throughput => "B/s",
    _ => "",
};

static void Esc(StringBuilder sb, string s)
{
    foreach (var c in s)
    {
        if (c == '"' || c == '\\') sb.Append('\\').Append(c);
        else if (c < ' ') sb.Append(' ');
        else sb.Append(c);
    }
}

static void Walk(IHardware hw, StringBuilder sb)
{
    hw.Update();
    foreach (var s in hw.Sensors)
    {
        if (s.Value is not float v || !float.IsFinite(v)) continue;
        if (sb[^1] != '[') sb.Append(',');
        sb.Append("{\"l\":\"");
        Esc(sb, hw.Name);
        sb.Append(" / ").Append(s.SensorType).Append("s / ");
        Esc(sb, s.Name);
        sb.Append("\",\"v\":").Append(v.ToString("R", CultureInfo.InvariantCulture))
          .Append(",\"u\":\"").Append(Unit(s.SensorType)).Append("\"}");
    }
    foreach (var sub in hw.SubHardware) Walk(sub, sb);
}

var buf = new StringBuilder(16 * 1024);
var stdout = Console.Out;
while (!quit.IsSet)
{
    try
    {
        buf.Clear().Append("{\"t\":[");
        foreach (var hw in pc.Hardware) Walk(hw, buf);
        buf.Append("]}");
        stdout.WriteLine(buf.ToString());
        stdout.Flush();
    }
    catch (IOException) { break; }
    catch (Exception e) { Console.Error.WriteLine(e.Message); }
    quit.Wait(intervalMs);
}
pc.Close();

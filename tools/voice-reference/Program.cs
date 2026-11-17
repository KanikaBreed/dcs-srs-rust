// No audio devices. Link unchanged original SRS codec wrappers; only native
// library resolution and the generated-signal test driver live in this project.
using System.Runtime.InteropServices;
using Ciribob.DCS.SimpleRadio.Standalone.Common.Audio.Opus;
using Ciribob.DCS.SimpleRadio.Standalone.Common.Audio.Opus.Core;

if (args.Length != 1) throw new ArgumentException("Usage: VoiceReference <fixture-directory>");
if (!BitConverter.IsLittleEndian) throw new PlatformNotSupportedException("Reference PCM is little-endian");
NativeLibrary.SetDllImportResolver(typeof(OpusEncoder).Assembly, (name, _, _) =>
    name == "opus" ? NativeLibrary.Load("libopus.so.0") : IntPtr.Zero);
Console.WriteLine("C# reference native codec: " + Marshal.PtrToStringAnsi(Native.opus_get_version_string()));
using var decoder = OpusDecoder.Create(48000, 1);
using var encoder = OpusEncoder.Create(16000, 1, Application.Voip);
encoder.ForwardErrorCorrection = false;
var root = Path.GetFullPath(args[0]);
var minimumFraction = 1.0;
for (var frame = 0; frame < 20; frame++)
{
    var input = File.ReadAllBytes(Path.Combine(root, $"rust-{frame:00}.opus"));
    if (input.Length == 0 || input.Length > 4000) throw new Exception("Invalid fixture size");
    var output = new short[1920];
    var count = decoder.DecodeShort(input, output, output.Length);
    if (count != 1920) throw new Exception("Rust packet was not 40 ms at 48 kHz");
    if (frame > 0)
    {
        double energy = 0, sine = 0, cosine = 0;
        for (var i = 0; i < count; i++)
        {
            energy += (double)output[i]*output[i];
            sine += output[i]*Math.Sin(2*Math.PI*700*i/48000.0);
            cosine += output[i]*Math.Cos(2*Math.PI*700*i/48000.0);
        }
        var fraction = 2*(sine*sine+cosine*cosine)/(count*energy);
        if (energy/count < 4_000_000 || !double.IsFinite(fraction) || fraction < 0.5)
            throw new Exception("Rust-encoded tone failed upstream decode/signal validation");
        minimumFraction = Math.Min(minimumFraction, fraction);
    }
    var pcm = new byte[640*2];
    for (var i = 0; i < 640; i++)
    {
        var sample = (short)(12000*Math.Sin(2*Math.PI*700*(frame*640+i)/16000.0));
        BitConverter.TryWriteBytes(pcm.AsSpan(i*2, 2), sample);
    }
    var encoded = encoder.Encode(pcm, pcm.Length, out var length);
    File.WriteAllBytes(Path.Combine(root, $"csharp-{frame:00}.opus"), encoded.AsSpan(0, length).ToArray());
}
Console.WriteLine($"PASS: original SRS C# decoded 20 Rust frames at 48 kHz/40 ms; minimum 700 Hz energy fraction {minimumFraction:F4}; wrote 20 C# frames");

internal static class Native
{
    [DllImport("opus", CallingConvention = CallingConvention.Cdecl)]
    internal static extern IntPtr opus_get_version_string();
}

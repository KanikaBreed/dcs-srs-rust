// Fixture-only non-wire shims. The upstream packet/model/serializer sources are
// linked unchanged. These replace logging and application version infrastructure.
namespace NLog
{
    public class Logger
    {
        public void Error(System.Exception error, string message) =>
            throw new System.InvalidOperationException(message, error);
    }
    public static class LogManager
    {
        public static Logger GetCurrentClassLogger() => new();
    }
}
namespace Ciribob.DCS.SimpleRadio.Standalone.Common.Helpers
{
    public static class UpdaterChecker
    {
        // Same expression as upstream; the harness assembly pins the fixture value.
        public static readonly string VERSION =
            System.Reflection.Assembly.GetEntryAssembly().GetName().Version.ToString();
    }
}

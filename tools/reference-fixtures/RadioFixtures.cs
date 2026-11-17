// Inputs are synthetic. Expected decisions come from unchanged upstream code.
using System.Text.Json;
using Ciribob.DCS.SimpleRadio.Standalone.Common.Models.Player;

static class RadioFixtures
{
    public static byte[] Create()
    {
        var cases = new List<object>();
        void Add(string name, RadioBase[] radios, double hz = 251000000,
            Modulation modulation = Modulation.AM, byte key = 0, bool strict = false,
            uint local = 42, uint sender = 42, int[] blocked = null)
        {
            var player = new PlayerRadioInfoBase { radios = radios, unitId = local };
            player.CanHearTransmission(hz, modulation, key, strict, sender,
                (blocked ?? []).ToList(), out var state, out var decryptable);
            cases.Add(new {
                name, radios = radios.Select(r => new { hz = r.freq, guard = r.secFreq,
                    modulation = (byte)r.modulation, key = r.enc ? (byte?)r.encKey : null,
                    intercom = r.IntercomUnitId }),
                hz, modulation = (byte)modulation, key, strict, local, sender,
                blocked = blocked ?? [],
                expected = state == null ? null : (object)new {
                    radio = state.ReceivedOn, secondary = state.IsSecondary, decryptable }
            });
        }
        RadioBase R(double hz = 251000000, double guard = 1, Modulation mod = Modulation.AM,
            byte? key = null, uint intercom = 0) => new() {
            freq = hz, secFreq = guard, modulation = mod, enc = key.HasValue,
            encKey = key ?? 0, IntercomUnitId = intercom };
        foreach (var delta in new double[] { -500, -499.9, 0, 499.9, 500 })
            Add($"tolerance-{delta.ToString(System.Globalization.CultureInfo.InvariantCulture)}", [R()], hz: 251000000 + delta);
        Add("disabled-rx", [R(mod: Modulation.DISABLED)]);
        Add("disabled-tx", [R()], modulation: Modulation.DISABLED);
        Add("primary-modulation", [R()], modulation: Modulation.FM);
        Add("guard-cross-modulation", [R(hz: 305000000, guard: 251000000)], modulation: Modulation.FM);
        Add("frequency-floor", [R(hz: 10000)], hz: 10000);
        Add("frequency-above-floor", [R(hz: 10001)], hz: 10001);
        foreach (var strict in new[] { false, true })
        foreach (var rx in new byte?[] { null, 0, 7, 8 })
        foreach (var tx in new byte[] { 0, 7, 8 })
            Add($"keys-{strict}-{rx}-{tx}", [R(key: rx)], key: tx, strict: strict);
        Add("later-decryptable", [R(key: 8), R(key: 7)], key: 7);
        Add("last-fallback", [R(key: 8), R(key: 9)], key: 7);
        Add("blocked-first", [R(), R()], blocked: [0]);
        Add("blocked-fallback", [R()], blocked: [0]);
        Add("blocked-guard", [R(hz: 305000000, guard: 251000000)], blocked: [0]);
        Add("intercom", [R(mod: Modulation.INTERCOM)], modulation: Modulation.INTERCOM, key: 7);
        Add("intercom-wrong-unit", [R(mod: Modulation.INTERCOM)], modulation: Modulation.INTERCOM, sender: 43);
        Add("intercom-zero", [R(mod: Modulation.INTERCOM)], modulation: Modulation.INTERCOM, local: 0, sender: 0);
        Add("intercom-override", [R(mod: Modulation.INTERCOM, intercom: 43)], modulation: Modulation.INTERCOM, sender: 43);
        Add("intercom-blocked", [R(mod: Modulation.INTERCOM)], modulation: Modulation.INTERCOM, blocked: [0]);
        // Known upstream bug: wrong-key guard inherits the previous blocked
        // primary's true decryptable flag. Rust intentionally corrects this.
        Add("guard-fallback-inherited-key", [R(key: 7), R(hz: 305000000, guard: 251000000, key: 8)], key: 7, blocked: [0]);
        return JsonSerializer.SerializeToUtf8Bytes(cases);
    }
}

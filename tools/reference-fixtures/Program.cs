// Synthetic inputs; packet and JSON output is produced by linked upstream code.
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Ciribob.DCS.SimpleRadio.Standalone.Common.Models;
using Ciribob.DCS.SimpleRadio.Standalone.Common.Models.Player;

if (args.Length != 2 || (args[0] != "--write" && args[0] != "--verify"))
    throw new ArgumentException("Usage: --write|--verify <fixture-directory>");
if (!BitConverter.IsLittleEndian)
    throw new PlatformNotSupportedException("The reference SRS wire representation is little-endian");

var fixtures = new Dictionary<string, byte[]>();
var id = "ufYS_WlLVkmFPjqCgxz6GA";
UDPVoicePacket Voice(byte hops, double[] frequencies, byte[] modulations, byte[] encryptions) => new()
{
    GuidBytes = Encoding.ASCII.GetBytes(id),
    OriginalClientGuidBytes = Encoding.ASCII.GetBytes(id),
    AudioPart1Bytes = new byte[] { 0, 1, 2, 3, 4, 5 },
    AudioPart1Length = 6,
    Frequencies = frequencies,
    Modulations = modulations,
    Encryptions = encryptions,
    UnitId = 1,
    PacketNumber = 1,
    RetransmissionCount = hops
};
fixtures["single-voice.bin"] = Voice(4, [100], [4], [0]).EncodePacket();
fixtures["single-relayed.bin"] = Voice(231, [100], [4], [0]).EncodePacket();
fixtures["multi-voice.bin"] = Voice(254, [251000000, 30000000, 251000000], [0, 1, 0], [0, 0, 1]).EncodePacket();
fixtures["keepalive.bin"] = Encoding.ASCII.GetBytes(id);
var distinct = Voice(255, [0, 251000000], [255, 7], [255, 0]);
distinct.GuidBytes = Encoding.ASCII.GetBytes("_____________________w");
distinct.OriginalClientGuidBytes = Encoding.ASCII.GetBytes("AAAAAAAAAAAAAAAAAAAAAA");
distinct.UnitId = uint.MaxValue;
distinct.PacketNumber = ulong.MaxValue;
fixtures["distinct-identities.bin"] = distinct.EncodePacket();

var client = new SRClientBase
{
    ClientGuid = id, Name = "Fixture TOC <&> \u00e9 \uD83D\uDE81", Coalition = 2,
    AllowRecord = false, Seat = 0, RadioInfo = new PlayerRadioInfoBase { unit = "Fixture TOC", unitId = 123 },
    LatLngPosition = new LatLngPosition { lat = 0, lng = 0, alt = 0 }
};
client.RadioInfo.radios[1] = new RadioBase
{
    freq = 251000000, secFreq = 243000000, modulation = Modulation.AM,
    enc = true, encKey = 7, Model = "ARC-164", Name = "Command"
};
void Message(string name, NetworkMessage message) => fixtures.Add(name, Encoding.UTF8.GetBytes(message.Encode()));
Message("ping.jsonl", new NetworkMessage { MsgType = NetworkMessage.MessageType.PING });
Message("sync.jsonl", new NetworkMessage
{
    MsgType = NetworkMessage.MessageType.SYNC, Clients = [client],
    ServerSettings = new() { ["EXTERNAL_AWACS_MODE"] = "true", ["COALITION_AUDIO_SECURITY"] = "true" }
});
Message("eam.jsonl", new NetworkMessage
{
    MsgType = NetworkMessage.MessageType.EXTERNAL_AWACS_MODE_PASSWORD,
    Client = new SRClientBase { ClientGuid = id, Name = "Fixture operator" },
    ExternalAWACSModePassword = "fixture-only-password"
});
Message("radio-update.jsonl", new NetworkMessage { MsgType = NetworkMessage.MessageType.RADIO_UPDATE, Client = client });
Message("version-mismatch.jsonl", new NetworkMessage { MsgType = NetworkMessage.MessageType.VERSION_MISMATCH });
fixtures["message-types.json"] = JsonSerializer.SerializeToUtf8Bytes(Enum.GetValues<NetworkMessage.MessageType>().ToDictionary(x => x.ToString(), x => (int)x));
fixtures["modulations.json"] = JsonSerializer.SerializeToUtf8Bytes(Enum.GetValues<Modulation>().ToDictionary(x => x.ToString(), x => (int)x));

var directory = Path.GetFullPath(args[1]);
if (args[0] == "--write") Directory.CreateDirectory(directory);
foreach (var (name, expected) in fixtures)
{
    var path = Path.Combine(directory, name);
    if (args[0] == "--write") { File.WriteAllBytes(path, expected); continue; }
    var actual = File.ReadAllBytes(path);
    if (name.EndsWith(".bin"))
    {
        if (!expected.SequenceEqual(actual)) throw new Exception("Reference bytes differ: " + name);
        if (actual.Length != 22 && UDPVoicePacket.DecodeVoicePacket(actual) is null)
            throw new Exception("Upstream could not decode: " + name);
    }
    else
    {
        if (!JsonNode.DeepEquals(JsonNode.Parse(expected), JsonNode.Parse(actual)))
            throw new Exception("Reference JSON differs: " + name);
        if (name.EndsWith(".jsonl"))
        {
            var decoded = JsonSerializer.Deserialize<NetworkMessage>(actual, new JsonSerializerOptions { IncludeFields = true });
            if (decoded is null || !actual.AsSpan().EndsWith(new byte[] { 10 }))
                throw new Exception("Invalid upstream control message: " + name);
        }
    }
}
Console.WriteLine($"{args[0]}: {fixtures.Count} upstream fixtures (synthetic inputs).");

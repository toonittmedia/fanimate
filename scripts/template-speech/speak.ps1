# Speak one line with a Windows voice into a WAV and write the mouth timing
# (SAPI visemes) beside it as JSON: [{ "t": seconds, "v": viseme 0-21 }].
# Used by gen-speech.mjs; needs Windows (System.Speech).
param(
    [Parameter(Mandatory)] [string] $Text,
    [Parameter(Mandatory)] [string] $Voice,
    [Parameter(Mandatory)] [string] $Wav,
    [Parameter(Mandatory)] [string] $Json,
    [string] $Rate = "medium",
    [string] $Pitch = 'medium'
)
Add-Type -AssemblyName System.Speech
Add-Type -ReferencedAssemblies System.Speech -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Speech.Synthesis;
using System.Speech.AudioFormat;
public static class Talker {
    public static string Run(string ssml, string wav) {
        var synth = new SpeechSynthesizer();
        var sb = new System.Text.StringBuilder("[");
        synth.VisemeReached += (s, e) => {
            if (sb.Length > 1) sb.Append(',');
            sb.Append("{\"t\":").Append(e.AudioPosition.TotalSeconds.ToString("0.000", System.Globalization.CultureInfo.InvariantCulture))
              .Append(",\"v\":").Append(e.Viseme).Append('}');
        };
        synth.SetOutputToWaveFile(wav, new SpeechAudioFormatInfo(16000, AudioBitsPerSample.Sixteen, AudioChannel.Mono));
        synth.SpeakSsml(ssml);
        synth.SetOutputToNull();
        synth.Dispose();
        return sb.Append(']').ToString();
    }
}
'@
$esc = [System.Security.SecurityElement]::Escape($Text)
$ssml = "<speak version='1.0' xmlns='http://www.w3.org/2001/10/synthesis' xml:lang='en-US'><voice name='$Voice'><prosody rate='$Rate' pitch='$Pitch'>$esc</prosody></voice></speak>"
[System.IO.File]::WriteAllText($Json, [Talker]::Run($ssml, $Wav))

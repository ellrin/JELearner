// jl-speech — Apple's on-device speech recognition for JELearner (docs/decisions.md §14).
//
//   jl-speech status <locale>...              → {"locales":[{"locale","supported","installed"}]}
//   jl-speech install <locale>                → {"locale","installed":true}  (downloads the model)
//   jl-speech transcribe <locale> <audio>     → {"text","confidence"}
//
// Every command prints one JSON object on stdout. Errors print
// {"error": "..."} and exit 1; the Rust side (jl_core::speech::MacSpeech)
// reads either. Audio is anything AVAudioFile reads (the web page sends
// 16 kHz mono WAV). Nothing leaves the machine.

import AVFoundation
import Foundation
import Speech

func emit(_ obj: [String: Any]) {
    let data = try! JSONSerialization.data(withJSONObject: obj, options: [.sortedKeys])
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write("\n".data(using: .utf8)!)
}

func fail(_ message: String) -> Never {
    emit(["error": message])
    exit(1)
}

/// The supported locale closest to what was asked (en → en-US, ja → ja-JP).
func resolve(_ tag: String) async -> Locale? {
    await SpeechTranscriber.supportedLocale(equivalentTo: Locale(identifier: tag))
}

func transcriber(for locale: Locale) -> SpeechTranscriber {
    SpeechTranscriber(locale: locale, transcriptionOptions: [], reportingOptions: [], attributeOptions: [.transcriptionConfidence])
}

func status(_ tags: [String]) async {
    let installed = Set(await SpeechTranscriber.installedLocales.map { $0.identifier(.bcp47) })
    var out: [[String: Any]] = []
    for tag in tags {
        if let loc = await resolve(tag) {
            let id = loc.identifier(.bcp47)
            out.append(["locale": id, "asked": tag, "supported": true, "installed": installed.contains(id)])
        } else {
            out.append(["locale": tag, "asked": tag, "supported": false, "installed": false])
        }
    }
    emit(["locales": out])
}

func install(_ tag: String) async {
    guard let loc = await resolve(tag) else { fail("這台電腦不支援 \(tag) 的語音辨識") }
    do {
        if let request = try await AssetInventory.assetInstallationRequest(supporting: [transcriber(for: loc)]) {
            try await request.downloadAndInstall()
        }
        emit(["locale": loc.identifier(.bcp47), "installed": true])
    } catch {
        fail("下載 \(tag) 語音模型失敗：\(error.localizedDescription)")
    }
}

func transcribe(_ tag: String, _ path: String) async {
    guard let loc = await resolve(tag) else { fail("這台電腦不支援 \(tag) 的語音辨識") }
    let installed = await SpeechTranscriber.installedLocales.map { $0.identifier(.bcp47) }
    guard installed.contains(loc.identifier(.bcp47)) else { fail("not_installed: \(loc.identifier(.bcp47))") }
    let file: AVAudioFile
    do {
        file = try AVAudioFile(forReading: URL(fileURLWithPath: path))
    } catch {
        fail("讀不了錄音檔：\(error.localizedDescription)")
    }
    let module = transcriber(for: loc)
    let analyzer = SpeechAnalyzer(modules: [module])
    do {
        // Collect final results while the analyzer runs over the file.
        async let collected: (String, [Double]) = {
            var text = ""
            var conf: [Double] = []
            for try await result in module.results where result.isFinal {
                text += String(result.text.characters)
                for run in result.text.runs {
                    if let c = run.transcriptionConfidence { conf.append(c) }
                }
            }
            return (text, conf)
        }()
        if let last = try await analyzer.analyzeSequence(from: file) {
            try await analyzer.finalizeAndFinish(through: last)
        } else {
            await analyzer.cancelAndFinishNow()
        }
        let (text, conf) = try await collected
        var out: [String: Any] = ["text": text.trimmingCharacters(in: .whitespacesAndNewlines), "locale": loc.identifier(.bcp47)]
        if !conf.isEmpty { out["confidence"] = conf.reduce(0, +) / Double(conf.count) }
        emit(out)
    } catch {
        fail("辨識失敗：\(error.localizedDescription)")
    }
}

let args = Array(CommandLine.arguments.dropFirst())
switch (args.first, args.count) {
case ("status", 2...): await status(Array(args.dropFirst()))
case ("install", 2): await install(args[1])
case ("transcribe", 3): await transcribe(args[1], args[2])
case ("--version", _): emit(["version": "1"])
default: fail("usage: jl-speech status <locale>... | install <locale> | transcribe <locale> <audio>")
}

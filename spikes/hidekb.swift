// Spike: can we hide a hardware MIDI source from other apps (GarageBand)
// while still reading it ourselves? Sets kMIDIPropertyPrivate on a source
// endpoint and restores it on Ctrl-C.
//
// Build and list sources:
//   swiftc spikes/hidekb.swift -o /tmp/hidekb && /tmp/hidekb
// Hide source N (Ctrl-C restores):
//   /tmp/hidekb N
//
// Pass criteria: with the keyboard hidden, a relaunched GarageBand no longer
// sounds the raw keyboard. Then verify restore works, and that recovery after
// `kill -9` is possible (delete the device in Audio MIDI Setup and replug).

import CoreMIDI
import Foundation

var client = MIDIClientRef()
MIDIClientCreate("hidekb" as CFString, nil, nil, &client)

let count = MIDIGetNumberOfSources()
if count == 0 {
    print("no MIDI sources found")
    exit(1)
}
for i in 0..<count {
    let src = MIDIGetSource(i)
    var name: Unmanaged<CFString>?
    MIDIObjectGetStringProperty(src, kMIDIPropertyDisplayName, &name)
    print("[\(i)] \(name?.takeRetainedValue() as String? ?? "?")")
}

guard CommandLine.arguments.count > 1, let idx = Int(CommandLine.arguments[1]), idx >= 0, idx < count else {
    print("\nusage: hidekb <index>   (hides that source until Ctrl-C)")
    exit(0)
}

let src = MIDIGetSource(idx)
let err = MIDIObjectSetIntegerProperty(src, kMIDIPropertyPrivate, 1)
if err != noErr {
    print("failed to set kMIDIPropertyPrivate: OSStatus \(err)")
    exit(1)
}
print("hidden. relaunch GarageBand, play the keyboard, then Ctrl-C here to restore.")

signal(SIGINT, SIG_IGN)
let sig = DispatchSource.makeSignalSource(signal: SIGINT)
sig.setEventHandler {
    let e = MIDIObjectSetIntegerProperty(src, kMIDIPropertyPrivate, 0)
    if e == noErr {
        print("\nrestored.")
    } else {
        print("\nrestore failed (OSStatus \(e)): delete the device in Audio MIDI Setup and replug.")
    }
    exit(0)
}
sig.resume()
RunLoop.main.run()

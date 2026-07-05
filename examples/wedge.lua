-- Mirror the keyboard around middle C (key 60).
--
-- Every note lands as far above the axis as it was played below it, and
-- vice versa: the C an octave down comes out an octave up. Note-offs
-- mirror the same way, so every mirrored note is released.

local axis = 60

function on_event(ev)
    if ev.kind == "note-on" or ev.kind == "note-off" then
        local key = 2 * axis - ev.key
        if key < 0 or key > 127 then
            return false -- mirrored past the end of the keyboard: drop
        end
        ev.key = key
        return ev
    end
    return nil -- everything else passes through untouched
end

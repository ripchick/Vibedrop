#!/bin/sh
# VibeDrop virtual microphone — the "CABLE Output" side of the virtual cable.
# Started by PipeWire itself via context.exec (see
# /usr/share/pipewire/pipewire.conf.d/90-vibedrop-cable.conf) so it always
# starts after the "VibeDrop_Route" sink exists; it is a child of the daemon
# and dies together with the pipewire.service cgroup, so it never outlives a
# restart. Everything played into the sink comes out of
# "VibeDrop Microphone (Virtual)", which voice chats select as their
# microphone.
#
# stream.capture.sink = true is essential: wireplumber 0.4.x resolves a
# capture stream's node.target only among nodes of the opposite direction
# (sources) unless this flag is set — without it the loopback silently
# captured the system default MICROPHONE instead of the cable sink.
#
# media.class stays "Audio/Source" (not "Audio/Source/Virtual"): on
# PipeWire 1.0.x the /Virtual suffix on a loopback playback stream produces a
# node with broken ports (one FR-only input port, no capture ports), while a
# plain Audio/Source yields proper stereo capture ports. The "(Virtual)" in
# the description still tells users it is not a hardware device.
exec pw-loopback \
    --capture-props='node.name="VibeDrop_Cable_Link" node.description="VibeDrop cable link" node.target="VibeDrop_Route" stream.capture.sink="true" node.passive="true"' \
    --playback-props='node.name="VibeDrop_Mic" node.description="VibeDrop Microphone (Virtual)" media.class="Audio/Source" node.priority.session="-1000"'

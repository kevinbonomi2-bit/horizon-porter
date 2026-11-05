package net.grind.net;

import net.neoforged.neoforge.network.event.RegisterPayloadHandlersEvent;
import net.neoforged.neoforge.network.registration.PayloadRegistrar;

public final class Network {
    public static void register(RegisterPayloadHandlersEvent event) {
        PayloadRegistrar registrar = event.registrar("1");
        registrar.playToServer(SetSpeed.TYPE, SetSpeed.STREAM_CODEC, (msg, ctx) -> {
            // Set the crusher's speed, if the player can reach it.
        });
        registrar.playToClient(Crushed.TYPE, Crushed.STREAM_CODEC, (msg, ctx) -> {
            // Show a toast on a milestone.
        });
    }
}

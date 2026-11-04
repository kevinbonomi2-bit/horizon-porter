package net.compasshud;

import com.mojang.blaze3d.platform.InputConstants;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.command.v2.ClientCommandManager;
import net.fabricmc.fabric.api.client.command.v2.ClientCommandRegistrationCallback;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.fabric.api.client.keybinding.v1.KeyBindingHelper;
import net.fabricmc.fabric.api.client.rendering.v1.HudRenderCallback;
import net.minecraft.client.KeyMapping;
import net.minecraft.network.chat.Component;
import org.lwjgl.glfw.GLFW;

public class CompassHudClient implements ClientModInitializer {
    static KeyMapping toggle;
    static KeyMapping cycle;
    static boolean shown = true;

    @Override
    public void onInitializeClient() {
        toggle = KeyBindingHelper.registerKeyBinding(new KeyMapping(
            "key.compasshud.toggle", InputConstants.Type.KEYSYM, GLFW.GLFW_KEY_H, "key.categories.compasshud"));
        cycle = KeyBindingHelper.registerKeyBinding(new KeyMapping(
            "key.compasshud.cycle", InputConstants.Type.KEYSYM, GLFW.GLFW_KEY_RIGHT_BRACKET, "key.categories.compasshud"));

        ClientTickEvents.END_CLIENT_TICK.register(client -> {
            while (toggle.consumeClick()) {
                shown = !shown;
            }
        });
        HudRenderCallback.EVENT.register((graphics, tick) -> CompassRenderer.draw(graphics, shown));
        ClientCommandRegistrationCallback.EVENT.register((dispatcher, context) ->
            dispatcher.register(ClientCommandManager.literal("compass").executes(c -> {
                c.getSource().sendFeedback(Component.literal("Compass shown: " + shown));
                return 1;
            })));
    }
}

package net.grind.client;

import net.grind.Grind;
import com.mojang.blaze3d.platform.InputConstants;
import net.minecraft.client.KeyMapping;
import net.minecraft.client.Minecraft;
import net.neoforged.neoforge.client.event.ClientTickEvent;
import net.neoforged.neoforge.client.event.EntityRenderersEvent;
import org.lwjgl.glfw.GLFW;

public final class GrindClient {
    public static final KeyMapping GUIDE = new KeyMapping("key.grind.guide", InputConstants.Type.KEYSYM, GLFW.GLFW_KEY_G, "key.categories.grind");

    public static void tick(ClientTickEvent.Post event) {
        while (GUIDE.consumeClick()) {
            Minecraft.getInstance().setScreen(new GuideScreen());
        }
    }

    public static void renderers(EntityRenderersEvent.RegisterRenderers event) {
        event.registerBlockEntityRenderer(Grind.CRUSHER_BE.get(), CrusherRenderer::new);
    }
}

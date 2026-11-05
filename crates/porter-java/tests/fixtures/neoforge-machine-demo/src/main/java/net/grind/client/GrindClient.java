package net.grind.client;

import net.grind.Grind;
import net.neoforged.neoforge.client.event.EntityRenderersEvent;

public final class GrindClient {
    public static void renderers(EntityRenderersEvent.RegisterRenderers event) {
        event.registerBlockEntityRenderer(Grind.CRUSHER_BE.get(), CrusherRenderer::new);
    }
}

package net.spin;

import com.mojang.blaze3d.vertex.PoseStack;
import com.mojang.math.Axis;
import net.minecraft.client.renderer.blockentity.BlockEntityRenderer;
import net.minecraft.client.renderer.blockentity.BlockEntityRendererProvider;

/** Turns the shaft's model at its speed (RPM): 6 degrees a tick at 1 RPM. */
public class KineticRenderer implements BlockEntityRenderer<KineticBlockEntity, KineticRenderer.State> {
    public KineticRenderer(BlockEntityRendererProvider.Context context) {}

    public void submit(State state, PoseStack pose) {
        float angle = (state.time * state.speed * 0.3f) % 360f;
        pose.pushPose();
        pose.rotateAround(Axis.YP.rotationDegrees(angle), 0.5f, 0.5f, 0.5f);
        pose.popPose();
    }

    public static class State {
        float speed;
        float time;
    }
}

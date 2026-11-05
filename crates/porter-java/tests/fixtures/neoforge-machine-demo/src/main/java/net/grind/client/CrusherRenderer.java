package net.grind.client;

import com.mojang.blaze3d.vertex.PoseStack;
import com.mojang.math.Axis;
import net.grind.machine.CrusherBlockEntity;
import net.minecraft.client.renderer.SubmitNodeCollector;
import net.minecraft.client.renderer.blockentity.BlockEntityRenderer;
import net.minecraft.client.renderer.blockentity.BlockEntityRendererProvider;
import net.minecraft.client.renderer.blockentity.state.BlockEntityRenderState;
import net.minecraft.client.renderer.state.CameraRenderState;
import net.minecraft.world.level.block.Blocks;

/** Spins a grinding wheel on top of the crusher while it works. */
public class CrusherRenderer implements BlockEntityRenderer<CrusherBlockEntity, CrusherRenderer.State> {
    public static class State extends BlockEntityRenderState {
        float progress;
        long gameTime;
        float partial;
    }

    public CrusherRenderer(BlockEntityRendererProvider.Context context) {}

    @Override
    public State createRenderState() {
        return new State();
    }

    @Override
    public void extractRenderState(CrusherBlockEntity crusher, State state, float partial,
                                   net.minecraft.world.phys.Vec3 camera,
                                   net.minecraft.client.renderer.feature.ModelFeatureRenderer.CrumblingOverlay crumbling) {
        BlockEntityRenderState.extractBase(crusher, state, crumbling);
        state.progress = crusher.progress;
        state.gameTime = crusher.getLevel().getGameTime();
        state.partial = partial;
    }

    @Override
    public void submit(State state, PoseStack pose, SubmitNodeCollector nodes, CameraRenderState camera) {
        if (state.progress <= 0) {
            return;
        }
        float angle = (state.gameTime + state.partial) * 18.0F % 360.0F;
        pose.pushPose();
        pose.translate(0.5F, 1.0F, 0.5F);
        pose.mulPose(Axis.YP.rotationDegrees(angle));
        pose.scale(0.5F, 0.125F, 0.5F);
        pose.translate(-0.5F, 0.0F, -0.5F);
        nodes.submitBlock(pose, Blocks.GRINDSTONE.defaultBlockState(), state.lightCoords, 0, 0);
        pose.popPose();
    }
}

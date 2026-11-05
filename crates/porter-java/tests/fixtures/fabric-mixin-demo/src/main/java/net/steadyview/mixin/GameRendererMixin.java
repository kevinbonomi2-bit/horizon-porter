package net.steadyview.mixin;

import net.minecraft.client.Camera;
import net.minecraft.client.renderer.GameRenderer;
import net.steadyview.SteadyView;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

@Mixin(GameRenderer.class)
public class GameRendererMixin {
    @Inject(method = "getFov", at = @At("RETURN"), cancellable = true)
    private void steadyview$steadyFov(Camera camera, float partialTick, boolean useSetting, CallbackInfoReturnable<Float> cir) {
        cir.setReturnValue(SteadyView.FOV);
    }
}

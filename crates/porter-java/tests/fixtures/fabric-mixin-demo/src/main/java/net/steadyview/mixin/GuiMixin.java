package net.steadyview.mixin;

import net.minecraft.client.DeltaTracker;
import net.minecraft.client.gui.Gui;
import net.minecraft.client.gui.GuiGraphics;
import net.steadyview.SteadyView;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(Gui.class)
public class GuiMixin {
    @Inject(method = "render", at = @At("TAIL"))
    private void steadyview$drawPickups(GuiGraphics graphics, DeltaTracker delta, CallbackInfo ci) {
        graphics.drawString(null, "Picked up: " + SteadyView.pickups, 4, 4, 0xFFFFFF);
    }
}

package net.steadyview.mixin;

import net.minecraft.world.entity.player.Player;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

@Mixin(Player.class)
public abstract class PlayerMixin {
    @Inject(method = "canEat", at = @At("HEAD"), cancellable = true)
    private void steadyview$noSneakEating(boolean canAlwaysEat, CallbackInfoReturnable<Boolean> cir) {
        if (((Player) (Object) this).isShiftKeyDown()) {
            cir.setReturnValue(false);
        }
    }
}

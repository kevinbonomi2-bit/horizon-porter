package net.shutters;

import net.minecraft.sounds.SoundEvents;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.InteractionResult;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.item.Item;
import net.minecraft.world.level.Level;

/** Blows a note. */
public class Whistle extends Item {
    public Whistle(Item.Properties properties) {
        super(properties);
    }

    @Override
    public InteractionResult use(Level level, Player player, InteractionHand hand) {
        level.playSound(null, player.blockPosition(), SoundEvents.NOTE_BLOCK_FLUTE.value(), player.getSoundSource(), 1.0f, 1.0f);
        return InteractionResult.SUCCESS;
    }
}

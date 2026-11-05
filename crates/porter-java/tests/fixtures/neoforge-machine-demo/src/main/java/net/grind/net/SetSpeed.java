package net.grind.net;

import net.grind.Grind;
import net.minecraft.core.BlockPos;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.codec.ByteBufCodecs;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.ResourceLocation;

/** A player sets a crusher's speed from its screen. */
public record SetSpeed(BlockPos pos, int speed) implements CustomPacketPayload {
    public static final Type<SetSpeed> TYPE =
        new Type<>(ResourceLocation.fromNamespaceAndPath(Grind.ID, "set_speed"));
    public static final StreamCodec<RegistryFriendlyByteBuf, SetSpeed> STREAM_CODEC = StreamCodec.composite(
        BlockPos.STREAM_CODEC, SetSpeed::pos,
        ByteBufCodecs.VAR_INT, SetSpeed::speed,
        SetSpeed::new);

    @Override
    public Type<SetSpeed> type() {
        return TYPE;
    }
}

package net.grind.net;

import net.grind.Grind;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.codec.ByteBufCodecs;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.ResourceLocation;

/** The server tells a player how many items their crushers have crushed. */
public record Crushed(long total, String item, boolean milestone) implements CustomPacketPayload {
    public static final Type<Crushed> TYPE =
        new Type<>(ResourceLocation.fromNamespaceAndPath(Grind.ID, "crushed"));
    public static final StreamCodec<FriendlyByteBuf, Crushed> STREAM_CODEC = StreamCodec.composite(
        ByteBufCodecs.VAR_LONG, Crushed::total,
        ByteBufCodecs.STRING_UTF8, Crushed::item,
        ByteBufCodecs.BOOL, Crushed::milestone,
        Crushed::new);

    @Override
    public Type<Crushed> type() {
        return TYPE;
    }
}

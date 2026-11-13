package net.dial;

import java.util.ArrayList;
import java.util.List;

import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.block.model.BakedQuad;
import net.minecraft.client.resources.model.BakedModel;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.BlockAndTintGetter;
import net.minecraft.world.level.block.state.BlockState;
import net.neoforged.neoforge.client.model.BakedModelWrapper;
import net.neoforged.neoforge.client.model.data.ModelData;
import net.neoforged.neoforge.client.model.data.ModelProperty;

/** The motor's model with the bracket its block entity keeps drawn on it, as Create's
 * BracketedKineticBlockModel adds one to a shaft. */
public class BracketModel extends BakedModelWrapper<BakedModel> {
    private static final ModelProperty<BlockState> BRACKET = new ModelProperty<>();

    public BracketModel(BakedModel template) {
        super(template);
    }

    @Override
    public ModelData getModelData(BlockAndTintGetter world, BlockPos pos, BlockState state, ModelData data) {
        if (world.getBlockEntity(pos) instanceof MotorBlockEntity be && be.bracket != null)
            return ModelData.builder().with(BRACKET, be.bracket).build();
        return data;
    }

    @Override
    public List<BakedQuad> getQuads(BlockState state, Direction side, RandomSource rand, ModelData data,
        net.minecraft.client.renderer.RenderType renderType) {
        List<BakedQuad> quads = new ArrayList<>(super.getQuads(state, side, rand, data, renderType));
        if (data.has(BRACKET))
            quads.addAll(Minecraft.getInstance().getBlockRenderer().getBlockModel(data.get(BRACKET))
                .getQuads(state, side, rand, data, renderType));
        return quads;
    }
}

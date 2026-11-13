package net.dial;

import java.util.List;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.phys.Vec3;

/** Turns at the speed set in the box on its side. */
public class MotorBlockEntity extends SmartBlockEntity {
    public static final int DEFAULT_SPEED = 16;
    public static final int MAX_SPEED = 256;

    public ScrollValueBehaviour generatedSpeed;
    public boolean reActivateSource;
    /** A bracket put on it with a wrench, drawn by its model (BracketModel). */
    public BlockState bracket;

    public MotorBlockEntity(BlockPos pos, BlockState state) {
        super(Dial.MOTOR_BE.get(), pos, state);
    }

    @Override
    public void addBehaviours(List<BlockEntityBehaviour> behaviours) {
        int max = MAX_SPEED;
        generatedSpeed = new KineticScrollValueBehaviour("dial.motor.speed", this, new MotorValueBox());
        generatedSpeed.between(-max, max);
        generatedSpeed.value = DEFAULT_SPEED;
        generatedSpeed.withCallback(i -> this.updateGeneratedRotation());
        behaviours.add(generatedSpeed);
    }

    public void updateGeneratedRotation() {
        reActivateSource = false;
    }

    static class MotorValueBox extends ValueBoxTransform.Sided {
        @Override
        protected Vec3 getSouthLocation() {
            return new Vec3(0.5, 0.5, 12.5 / 16);
        }

        @Override
        protected boolean isSideActive(Direction direction) {
            return direction.getAxis().isHorizontal();
        }
    }
}

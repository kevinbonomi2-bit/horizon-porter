package net.dial;

import net.minecraft.core.Direction;
import net.minecraft.world.phys.Vec3;

/** Where a value's box is on its block. */
public abstract class ValueBoxTransform {
    public abstract Vec3 getLocalOffset(Direction side);

    public static abstract class Sided extends ValueBoxTransform {
        protected abstract Vec3 getSouthLocation();

        @Override
        public Vec3 getLocalOffset(Direction side) {
            return getSouthLocation();
        }

        protected boolean isSideActive(Direction direction) {
            return true;
        }
    }
}

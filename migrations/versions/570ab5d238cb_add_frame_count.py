"""Add frame count

Revision ID: 570ab5d238cb
Revises: f1fcaab724c8
Create Date: 2024-04-24 11:12:00.092705

"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa
import sqlmodel


# revision identifiers, used by Alembic.
revision: str = '570ab5d238cb'
down_revision: Union[str, None] = 'f1fcaab724c8'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    # ### commands auto generated by Alembic - please adjust! ###
    op.add_column('inputobject', sa.Column('frame_count', sa.Integer(), nullable=True))
    # ### end Alembic commands ###


def downgrade() -> None:
    # ### commands auto generated by Alembic - please adjust! ###
    op.drop_column('inputobject', 'frame_count')
    # ### end Alembic commands ###
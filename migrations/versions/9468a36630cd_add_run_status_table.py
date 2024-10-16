"""Add run status table

Revision ID: 9468a36630cd
Revises: a153c3f14df3
Create Date: 2024-10-18 11:57:44.466397

"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa
import sqlmodel


# revision identifiers, used by Alembic.
revision: str = '9468a36630cd'
down_revision: Union[str, None] = 'a153c3f14df3'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    # ### commands auto generated by Alembic - please adjust! ###
    op.create_table('runstatus',
    sa.Column('submission_id', sqlmodel.sql.sqltypes.GUID(), nullable=False),
    sa.Column('kubernetes_pod_name', sqlmodel.sql.sqltypes.AutoString(), nullable=False),
    sa.Column('status', sqlmodel.sql.sqltypes.AutoString(), nullable=True),
    sa.Column('is_running', sa.Boolean(), nullable=False),
    sa.Column('is_successful', sa.Boolean(), nullable=False),
    sa.Column('is_still_kubernetes_resource', sa.Boolean(), nullable=False),
    sa.Column('time_started', sqlmodel.sql.sqltypes.AutoString(), nullable=True),
    sa.Column('logs', sa.JSON(), nullable=True),
    sa.Column('time_added_utc', sa.DateTime(), nullable=False),
    sa.Column('last_updated', sa.DateTime(), server_default=sa.text('now()'), nullable=False),
    sa.Column('id', sqlmodel.sql.sqltypes.GUID(), nullable=False),
    sa.ForeignKeyConstraint(['submission_id'], ['submission.id'], ),
    sa.PrimaryKeyConstraint('id')
    )
    op.create_index(op.f('ix_runstatus_id'), 'runstatus', ['id'], unique=False)
    op.create_index(op.f('ix_runstatus_is_running'), 'runstatus', ['is_running'], unique=False)
    op.create_index(op.f('ix_runstatus_is_still_kubernetes_resource'), 'runstatus', ['is_still_kubernetes_resource'], unique=False)
    op.create_index(op.f('ix_runstatus_is_successful'), 'runstatus', ['is_successful'], unique=False)
    op.create_index(op.f('ix_runstatus_kubernetes_pod_name'), 'runstatus', ['kubernetes_pod_name'], unique=False)
    op.create_index(op.f('ix_runstatus_status'), 'runstatus', ['status'], unique=False)
    op.create_index(op.f('ix_runstatus_submission_id'), 'runstatus', ['submission_id'], unique=False)
    op.create_index(op.f('ix_runstatus_time_added_utc'), 'runstatus', ['time_added_utc'], unique=False)
    op.create_index(op.f('ix_runstatus_time_started'), 'runstatus', ['time_started'], unique=False)
    # ### end Alembic commands ###


def downgrade() -> None:
    # ### commands auto generated by Alembic - please adjust! ###
    op.drop_index(op.f('ix_runstatus_time_started'), table_name='runstatus')
    op.drop_index(op.f('ix_runstatus_time_added_utc'), table_name='runstatus')
    op.drop_index(op.f('ix_runstatus_submission_id'), table_name='runstatus')
    op.drop_index(op.f('ix_runstatus_status'), table_name='runstatus')
    op.drop_index(op.f('ix_runstatus_kubernetes_pod_name'), table_name='runstatus')
    op.drop_index(op.f('ix_runstatus_is_successful'), table_name='runstatus')
    op.drop_index(op.f('ix_runstatus_is_still_kubernetes_resource'), table_name='runstatus')
    op.drop_index(op.f('ix_runstatus_is_running'), table_name='runstatus')
    op.drop_index(op.f('ix_runstatus_id'), table_name='runstatus')
    op.drop_table('runstatus')
    # ### end Alembic commands ###
